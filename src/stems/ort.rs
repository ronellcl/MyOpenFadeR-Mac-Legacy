//! The part of ONNX Runtime's C API that STEMS uses, loaded at run time with `libloading`.
//!
//! ONNX Runtime 1.16.3 is the newest release whose macOS x86_64 dylib runs on 10.15 Catalina (`minos 10.14`; 1.17 needs 11.0,
//! 1.20 needs 13.3), and the Rust wrapper for it — `ort` 1.16 and its `ort-sys` — is yanked from crates.io with its artifacts
//! gone. This build therefore calls the C API itself. The types come from `ort-sys` 2.0, whose `OrtApi` is the version-17
//! struct; ONNX Runtime 1.16.3 hands out the version-16 struct, a prefix of it, so every function pointer used here keeps its
//! offset. `GetApi(16)` is what asks for it.
//!
//! Nothing is linked: [`Runtime::load`] opens the dylib named by `$ORT_DYLIB_PATH` (the bundle's, see
//! [`super::model::init_ort`]), so building needs no ONNX Runtime at all and the link stays free of the CoreML symbols that
//! force LTO off in the main build.

use ort_sys as sys;
use std::ffi::{CStr, c_void};
use std::path::Path;
use std::ptr;
use std::sync::LazyLock;

/// The C API version ONNX Runtime 1.16 implements.
const API_VERSION: u32 = 16;

/// ONNX Runtime, its environment and the library that holds them, once per process.
pub struct Runtime {
    api: &'static sys::OrtApi,
    env: *mut sys::OrtEnv,
    /// The version ONNX Runtime reports, for the diagnostics of a failed open.
    version: String,
    _lib: libloading::Library,
}

// SAFETY: ONNX Runtime's environment is thread-safe, and the library is only unloaded with the process.
unsafe impl Send for Runtime {}
unsafe impl Sync for Runtime {}

/// The process's ONNX Runtime, loaded on first use (an `Err` is remembered: no dylib, or one too old).
static RUNTIME: LazyLock<Result<Runtime, String>> = LazyLock::new(Runtime::load);

/// The process's ONNX Runtime, or why there is none.
pub fn runtime() -> Result<&'static Runtime, String> {
    RUNTIME.as_ref().map_err(Clone::clone)
}

impl Runtime {
    fn load() -> Result<Runtime, String> {
        // SAFETY: `Runtime` is built from what the library returns and only used through the pointers it hands out; the
        // library outlives them (`_lib` is dropped last, with the process).
        unsafe {
            let (lib, path) = open()?;
            let base_getter: libloading::Symbol<
                unsafe extern "system" fn() -> *const sys::OrtApiBase,
            > = lib
                .get(b"OrtGetApiBase\0")
                .map_err(|e| format!("{}: not ONNX Runtime ({e})", path.display()))?;
            let base = &*base_getter();
            let api = (base.GetApi)(API_VERSION);
            if api.is_null() {
                let have = CStr::from_ptr((base.GetVersionString)()).to_string_lossy();
                return Err(format!(
                    "{}: ONNX Runtime {have} is older than 1.16 (API {API_VERSION})",
                    path.display()
                ));
            }
            let version = CStr::from_ptr((base.GetVersionString)())
                .to_string_lossy()
                .into_owned();
            let mut env = ptr::null_mut();
            let log_id = c"openfader-stems";
            status(
                &*api,
                ((*api).CreateEnv)(
                    sys::OrtLoggingLevel::ORT_LOGGING_LEVEL_ERROR,
                    log_id.as_ptr(),
                    &mut env,
                ),
            )?;
            Ok(Runtime {
                api: &*api,
                env,
                version,
                _lib: lib,
            })
        }
    }

    /// The version ONNX Runtime reports.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Opens the model at `path`: one session, `threads` intra-op threads, `level` graph optimizations, no CPU arena.
    pub fn session(
        &self,
        path: &Path,
        threads: usize,
        level: sys::GraphOptimizationLevel,
    ) -> Result<Session, String> {
        // SAFETY: the options are released on every path below; the session owns what ONNX Runtime allocated for it.
        unsafe {
            let api = self.api;
            let mut opts = ptr::null_mut();
            status(api, (api.CreateSessionOptions)(&mut opts))?;
            let opened = (|| {
                status(api, (api.SetSessionGraphOptimizationLevel)(opts, level))?;
                status(
                    api,
                    (api.SetIntraOpNumThreads)(opts, i32::try_from(threads).unwrap_or(i32::MAX)),
                )?;
                status(api, (api.SetInterOpNumThreads)(opts, 1))?;
                // One session per separation and two engines share the machine: the arena keeps every peak.
                status(api, (api.DisableCpuMemArena)(opts))?;
                // Idle intra-op threads spin by default; two engines share the machine, so let them sleep.
                status(
                    api,
                    (api.AddSessionConfigEntry)(
                        opts,
                        c"session.intra_op.allow_spinning".as_ptr(),
                        c"0".as_ptr(),
                    ),
                )?;
                let model = os_path(path)?;
                let mut raw = ptr::null_mut();
                status(
                    api,
                    (api.CreateSession)(self.env, model.as_ptr(), opts, &mut raw),
                )?;
                let mut mem = ptr::null_mut();
                status(
                    api,
                    (api.CreateCpuMemoryInfo)(
                        sys::OrtAllocatorType::OrtDeviceAllocator,
                        sys::OrtMemType::OrtMemTypeDefault,
                        &mut mem,
                    ),
                )?;
                Ok(Session { api, raw, mem })
            })();
            (api.ReleaseSessionOptions)(opts);
            opened
        }
    }
}

/// An open session: the model, its weights and its arena, for the lifetime of the process.
pub struct Session {
    api: &'static sys::OrtApi,
    raw: *mut sys::OrtSession,
    mem: *mut sys::OrtMemoryInfo,
}

// SAFETY: ONNX Runtime sessions may be run from any thread, one run at a time (`Session::run` takes `&mut self`).
unsafe impl Send for Session {}

impl Session {
    /// Runs the model on `mix` (the input tensor's data, `[1, 2, SEGMENT]`), returning the output's shape and data.
    pub fn run(&mut self, mix: &[f32]) -> Result<(Vec<i64>, Vec<f32>), String> {
        // SAFETY: every value made here is released before returning; ONNX Runtime reads `mix` only during `Run`.
        unsafe {
            let api = self.api;
            let shape = [1i64, 2, (mix.len() / 2) as i64];
            let mut input = ptr::null_mut();
            status(
                api,
                (api.CreateTensorWithDataAsOrtValue)(
                    self.mem,
                    mix.as_ptr().cast_mut().cast::<c_void>(),
                    std::mem::size_of_val(mix),
                    shape.as_ptr(),
                    shape.len(),
                    sys::ONNXTensorElementDataType::ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT,
                    &mut input,
                ),
            )?;
            let names_in = [c"mix".as_ptr()];
            let values_in = [input as *const sys::OrtValue];
            let names_out = [c"stems".as_ptr()];
            let mut output = ptr::null_mut();
            let run = (api.Run)(
                self.raw,
                ptr::null(),
                names_in.as_ptr(),
                values_in.as_ptr(),
                names_in.len(),
                names_out.as_ptr(),
                names_out.len(),
                &mut output,
            );
            (api.ReleaseValue)(input);
            status(api, run)?;
            let out = read(api, output);
            (api.ReleaseValue)(output);
            out
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: both pointers are ours and released once.
        unsafe {
            (self.api.ReleaseSession)(self.raw);
            (self.api.ReleaseMemoryInfo)(self.mem);
        }
    }
}

/// The shape and data of an output tensor.
///
/// # Safety
/// `value` is a tensor of f32 (the model's output) still owned by the caller, who releases it.
unsafe fn read(
    api: &'static sys::OrtApi,
    value: *mut sys::OrtValue,
) -> Result<(Vec<i64>, Vec<f32>), String> {
    unsafe {
        let mut info = ptr::null_mut();
        status(api, (api.GetTensorTypeAndShape)(value, &mut info))?;
        let read = (|| {
            let mut dims = 0usize;
            status(api, (api.GetDimensionsCount)(info, &mut dims))?;
            let mut shape = vec![0i64; dims];
            status(api, (api.GetDimensions)(info, shape.as_mut_ptr(), dims))?;
            let mut count = 0usize;
            status(api, (api.GetTensorShapeElementCount)(info, &mut count))?;
            let mut data: *mut c_void = ptr::null_mut();
            status(api, (api.GetTensorMutableData)(value, &mut data))?;
            let data = std::slice::from_raw_parts(data.cast::<f32>(), count).to_vec();
            Ok((shape, data))
        })();
        (api.ReleaseTensorTypeAndShapeInfo)(info);
        read
    }
}

fn status(api: &'static sys::OrtApi, status: sys::OrtStatusPtr) -> Result<(), String> {
    let ptr = status.0;
    if ptr.is_null() {
        return Ok(());
    }
    // SAFETY: a non-null status is ONNX Runtime's to describe and to release.
    unsafe {
        let msg = CStr::from_ptr((api.GetErrorMessage)(ptr))
            .to_string_lossy()
            .into_owned();
        (api.ReleaseStatus)(ptr);
        Err(msg)
    }
}

/// The library to load: `$ORT_DYLIB_PATH`, else the usual names next to the program and in `../Frameworks`, else the name
/// alone (the system's, found through the loader's search path).
fn open() -> Result<(libloading::Library, std::path::PathBuf), String> {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    let mut names = vec![
        "libonnxruntime.1.16.3.dylib",
        "libonnxruntime.dylib",
        "libonnxruntime.so.1.16.3",
        "libonnxruntime.so",
        "onnxruntime.dll",
    ];
    let mut paths = Vec::new();
    if let Some(dir) = exe {
        for n in &names {
            paths.push(dir.join(n));
            paths.push(dir.join("../Frameworks").join(n));
        }
    }
    if let Some(named) = std::env::var_os("ORT_DYLIB_PATH").filter(|v| !v.is_empty()) {
        paths.insert(0, named.into());
    }
    paths.push(names.remove(0).into()); // the loader's own search path, as a last resort
    let mut last = String::new();
    for p in paths {
        // SAFETY: `Library::new` is unsafe only because arbitrary code runs on load; this is ONNX Runtime by name.
        match unsafe { libloading::Library::new(&p) } {
            Ok(lib) => return Ok((lib, p)),
            Err(e) => last = format!("{}: {e}", p.display()),
        }
    }
    Err(format!("no ONNX Runtime found ({last})"))
}

/// The path as the C string ONNX Runtime reads (UTF-16 on Windows).
#[cfg(not(windows))]
use std::ffi::CString;

#[cfg(not(windows))]
fn os_path(path: &Path) -> Result<CString, String> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| format!("{}: the path holds a NUL byte", path.display()))
}

#[cfg(windows)]
fn os_path(path: &Path) -> Result<Vec<u16>, String> {
    use std::os::windows::ffi::OsStrExt;
    Ok(path.as_os_str().encode_wide().chain(Some(0)).collect())
}
