//! The separation model: Demucs v4 `htdemucs` (Meta, MIT licence), in the ONNX export published by StemSplit
//! (`StemSplitio/htdemucs-onnx`, MIT) with the STFT/iSTFT inside the graph. Run by ONNX Runtime 1.16.3 (`ort` 1.16) on the
//! CPU ([`Engine`]); this is the Catalina build, whose ONNX Runtime has no WebGPU provider, see `LEGACY.md`.
//!
//! The dylib is not linked but loaded at run time (`ort`'s `load-dynamic`): [`init_ort`] points ONNX Runtime at the one in the
//! bundle, or `$ORT_DYLIB_PATH` names it.
//!
//! The weights are never in this repository: [`path`] is `$OPENFADER_STEMS_MODEL`, else
//! `$XDG_CACHE_HOME/openfader/models/htdemucs.onnx`, and [`fetch`] downloads it there (316 MB). By hand:
//! `curl -L --create-dirs -o ~/.cache/openfader/models/htdemucs.onnx <URL>` (sha256 in [`SHA256`]).
//!
//! Interface (observed from the model card and by running it): input `mix` `[1, 2, 343980]` f32 (stereo, 44.1 kHz,
//! 7.8 s), output `stems` `[1, 4, 2, 343980]` in the order [`ORDER`]. Fixed segment length: longer audio is chunked
//! by [`super::separate`].

use super::ort;
use crate::analysis::cache;
use parking_lot::Mutex;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

pub const URL: &str = "https://huggingface.co/StemSplitio/htdemucs-onnx/resolve/main/htdemucs.onnx";
pub const SIZE: u64 = 316_446_953;
pub const SHA256: &str = "68d0bf16428ef66e692cdff8a9ccf28f1ef3f69440d57e58605a4cc55fcc5e74";
pub const RATE: u32 = 44_100;
pub const SEGMENT: usize = 343_980;
const MODEL_ENV: &str = "OPENFADER_STEMS_MODEL";
/// Our stem (`audio::VOCAL`…) of each model output, in the model's order: drums, bass, other, vocals.
pub const ORDER: [usize; 4] = [
    crate::audio::DRUMS,
    crate::audio::BASS,
    crate::audio::INST,
    crate::audio::VOCAL,
];

/// Where the model file is (or goes).
pub fn path() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os(MODEL_ENV).filter(|v| !v.is_empty()) {
        return Ok(p.into());
    }
    Ok(cache::root()
        .ok_or("no cache directory (set XDG_CACHE_HOME or HOME)")?
        .join("models")
        .join("htdemucs.onnx"))
}

/// The model file exists with the published size (a cut-off download does not count).
pub fn present() -> bool {
    path()
        .ok()
        .and_then(|p| std::fs::metadata(p).ok())
        .is_some_and(|m| m.len() == SIZE)
}

/// A separation can start without a download: the model is in place, or `$OPENFADER_STEMS_MODEL` names one (whose size is not
/// checked).
pub fn usable() -> bool {
    present() || std::env::var_os(MODEL_ENV).is_some()
}

/// The message the UI shows while the model is missing.
pub fn missing_hint() -> String {
    format!(
        "STEMS model missing: turn STEMS on to download it ({} MB), or `curl -L --create-dirs -o {} {URL}`",
        SIZE / 1_000_000,
        path().map_or_else(|e| e, |p| p.display().to_string())
    )
}

/// Bytes of the download in progress; `u64::MAX` while none runs.
static GOT: AtomicU64 = AtomicU64::new(u64::MAX);

/// The bytes downloaded so far while [`fetch`] runs (of [`SIZE`]), from any thread.
pub fn downloading() -> Option<u64> {
    Some(GOT.load(Relaxed)).filter(|&g| g != u64::MAX)
}

/// Downloads the model to [`path`] (via a `.part` file, so a failure never leaves a truncated model), reporting 0..=1.
/// Offline or a short download is an error naming what to do by hand. One download at a time: a second caller waits
/// for the first and finds the model in place.
pub fn fetch(progress: &dyn Fn(f32)) -> Result<(), String> {
    static ONE: Mutex<()> = Mutex::new(());
    let _one = ONE.lock();
    if present() {
        return Ok(());
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            GOT.store(u64::MAX, Relaxed);
        }
    }
    GOT.store(0, Relaxed);
    let _done = Done;
    let dest = path()?;
    let hand = |e: String| {
        format!(
            "STEMS model download failed ({e}); fetch it by hand: curl -L --create-dirs -o {} {URL}",
            dest.display()
        )
    };
    std::fs::create_dir_all(dest.parent().ok_or("model path has no folder")?)
        .map_err(|e| hand(e.to_string()))?;
    let resp = ureq::get(URL).call().map_err(|e| hand(e.to_string()))?;
    let mut body = resp.into_body().into_reader();
    let part = dest.with_extension("onnx.part");
    let mut out =
        std::io::BufWriter::new(std::fs::File::create(&part).map_err(|e| hand(e.to_string()))?);
    let (mut buf, mut got) = (vec![0u8; 1 << 16], 0u64);
    loop {
        let n = body.read(&mut buf).map_err(|e| hand(e.to_string()))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| hand(e.to_string()))?;
        got += n as u64;
        GOT.store(got, Relaxed);
        progress((got as f32 / SIZE as f32).min(1.0));
    }
    out.flush().map_err(|e| hand(e.to_string()))?;
    drop(out);
    if got != SIZE {
        let _ = std::fs::remove_file(&part);
        return Err(hand(format!("got {got} of {SIZE} bytes")));
    }
    std::fs::rename(&part, &dest).map_err(|e| hand(e.to_string()))
}

/// Points ONNX Runtime at the dylib of the bundle (`ort`'s `load-dynamic` loads it on first use): `$ORT_DYLIB_PATH` when it is
/// set, else the newest `libonnxruntime` next to the program or in `../Frameworks` (the macOS bundle). Nothing found is not an
/// error: `ort` then looks for a system ONNX Runtime the way it does by default.
///
/// Call it once at startup, before any thread exists to read the environment.
pub fn init_ort() {
    if std::env::var_os("ORT_DYLIB_PATH").is_some_and(|v| !v.is_empty()) {
        return;
    }
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(std::path::Path::to_path_buf))
    else {
        return;
    };
    let names = [
        "libonnxruntime.1.16.3.dylib",
        "libonnxruntime.dylib",
        "libonnxruntime.so.1.16.3",
        "libonnxruntime.so",
    ];
    let found = names
        .iter()
        .flat_map(|n| [dir.join(n), dir.join("../Frameworks").join(n)])
        .find(|p| p.is_file());
    if let Some(lib) = found {
        // SAFETY: the first thing main does, before any thread exists to read the environment.
        unsafe { std::env::set_var("ORT_DYLIB_PATH", lib) };
    }
}

/// ONNX Runtime's graph optimizations: **none**, which is not a mistake.
///
/// 1.16 folds the in-graph STFT into 2.2 GB of constants with its basic level, and it has neither the
/// `optimization.disable_specified_optimizers` option nor a per-provider arena switch to turn just that pass off (the main
/// build uses both). Measured on one 8-core machine, one 60 s track, ORT 1.16.3: no optimizations 70 s and 1.2 GB RSS, basic
/// (folded) 71 s and 4.3 GB, all levels 63 s and 4.3 GB — the fold costs memory and buys nothing here.
/// `OPENFADER_STEMS_OPT=1|2|all` opts into the folded levels anyway, to compare on a machine with memory to spare.
fn optimization_level() -> ort_sys::GraphOptimizationLevel {
    match std::env::var("OPENFADER_STEMS_OPT").as_deref() {
        Ok("1") | Ok("basic") => ort_sys::GraphOptimizationLevel::ORT_ENABLE_BASIC,
        Ok("2") | Ok("extended") => ort_sys::GraphOptimizationLevel::ORT_ENABLE_EXTENDED,
        Ok("3") | Ok("all") => ort_sys::GraphOptimizationLevel::ORT_ENABLE_ALL,
        _ => ort_sys::GraphOptimizationLevel::ORT_DISABLE_ALL,
    }
}

/// An open model: one ONNX Runtime session.
pub struct Model(ort::Session);

/// Hands freed heap pages back to the system (glibc keeps them otherwise).
pub(crate) fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        // SAFETY: glibc's malloc_trim only releases free memory; any argument is valid.
        unsafe { malloc_trim(0) };
    }
}

impl Model {
    /// Memory: the main build turns constant folding off (a 1.17+ session option) and keeps the CPU arena out, which is 1.4 GB
    /// on a 60 s track here. This build gets the same session (no arena, no optimizations) through [`optimization_level`]:
    /// 1.2 GB, and the same stems in the same time.
    fn open(threads: usize) -> Result<Model, String> {
        let p = path()?;
        let model = ort::runtime()?
            .session(&p, threads, optimization_level())
            .map(Model);
        let model = crate::paths::at(model, &p);
        trim_heap(); // what the graph transformations allocated and freed
        model
    }

    /// One segment: `mix` is `[L; SEGMENT]` then `[R; SEGMENT]`; returns `[model stem][channel][SEGMENT]` flattened.
    fn run(&mut self, mix: Vec<f32>) -> Result<Vec<f32>, String> {
        let (shape, data) = self.0.run(&mix)?;
        if shape != [1, 4, 2, SEGMENT as i64] {
            return Err(format!("unexpected model output shape {shape:?}"));
        }
        Ok(data)
    }
}

/// Where a segment runs. The CPU is ONNX Runtime's default provider and the only one this build has: ONNX Runtime 1.16.3 (the
/// newest release that runs on 10.15, `LEGACY.md`) has no WebGPU provider, so [`Engine::Gpu`] stays in the enum for the engine
/// picking code and is always out. The session lives for the whole process and is shared by every deck's separation: opening
/// it costs about 2 s.
///
/// Measured 2026-10-13 on a Ryzen/RTX 3090 box under load (`examples/stems_bench.rs`, one 7.8 s segment): CPU 1.15 to 1.45 s
/// whatever the thread count (8, 12 or 16; the graph does not scale).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Engine {
    /// Out of this build: no WebGPU provider.
    Gpu,
    /// ONNX Runtime intra-op threads.
    Cpu(usize),
}

/// A lazily opened session of an engine that may turn out unusable: the reason is remembered and logged once, and the engine
/// is never tried again.
enum Slot<T> {
    Untried,
    Ready(T),
    Unusable(String),
}

impl<T> Slot<T> {
    /// The session, opened by `open` on the first call (not at all when `allowed` is `Err(why)`); `log` hears the reason once
    /// if the engine is out.
    fn ensure(
        &mut self,
        allowed: Result<(), &str>,
        open: impl FnOnce() -> Result<T, String>,
        log: impl FnOnce(&str),
    ) -> Result<&mut T, String> {
        if let Slot::Untried = self {
            *self = match allowed {
                Err(why) => Slot::Unusable(why.into()),
                Ok(()) => open().map_or_else(Slot::Unusable, Slot::Ready),
            };
            if let Slot::Unusable(why) = self {
                log(why);
            }
        }
        match self {
            Slot::Ready(t) => Ok(t),
            Slot::Unusable(why) => Err(why.clone()),
            Slot::Untried => unreachable!("opened above"),
        }
    }
}

static GPU: Mutex<Slot<Model>> = Mutex::new(Slot::Untried);
static CPU: Mutex<Option<(usize, Model)>> = Mutex::new(None);

/// The GPU is out of this build: ONNX Runtime 1.16.3 has no WebGPU provider. The reason is remembered once, like a machine
/// without Vulkan, so the engine picking code needs no special case.
fn gpu_session() -> Result<(), String> {
    GPU.lock()
        .ensure(
            Err("not in this build: ONNX Runtime 1.16.3 has no WebGPU provider"),
            || unreachable!("no GPU session in this build"),
            |why| eprintln!("stems: GPU unavailable ({why}); separating on the CPU"),
        )
        .map(|_| ())
}

/// Closes the session before the process exits. Waits for a segment in flight; engines opened afterwards are out.
/// (The main build does the same for Dawn, whose teardown inside `exit()` crashes with a session still open.)
pub fn shutdown() {
    *GPU.lock() = Slot::Unusable("shut down".into());
    *CPU.lock() = None;
}

/// Closes the open sessions to give their memory back (about 1 GB for both); the next run opens them again (2 to 3 s). An
/// engine found unusable stays out. Waits for a segment in flight.
pub fn release() {
    let mut g = GPU.lock();
    if matches!(*g, Slot::Ready(_)) {
        *g = Slot::Untried;
    }
    drop(g);
    *CPU.lock() = None;
    trim_heap();
}

impl Engine {
    pub fn label(self) -> &'static str {
        match self {
            Engine::Gpu => "GPU",
            Engine::Cpu(_) => "CPU",
        }
    }

    /// Opens the engine's session now if it is not open (the GPU: the first time per process). `Err` = this engine is out,
    /// for the GPU with the reason logged once.
    pub fn open(self) -> Result<(), String> {
        if !usable() {
            return Err(missing_hint()); // not the engine's fault: nothing is remembered
        }
        match self {
            Engine::Gpu => gpu_session(),
            Engine::Cpu(threads) => {
                let mut c = CPU.lock();
                if c.as_ref().is_none_or(|(t, _)| *t != threads) {
                    *c = None; // the old session goes first: two copies of the weights are 600 MB
                    *c = Some((threads, Model::open(threads)?));
                }
                Ok(())
            }
        }
    }

    /// One segment ([`Model::run`]'s layout) on this engine, opening it first if needed.
    pub fn run(self, mix: Vec<f32>) -> Result<Vec<f32>, String> {
        self.open()?;
        match self {
            Engine::Gpu => Err("GPU unavailable".into()), // `open` never gets this far: no provider in this build
            Engine::Cpu(threads) => {
                let mut c = CPU.lock();
                match &mut *c {
                    Some((t, m)) if *t == threads => m.run(mix),
                    _ => Err("CPU session replaced".into()),
                }
            }
        }
    }
}

/// Opens the sessions in the background, so the first track loaded after STEMS is switched on does not wait for them.
pub fn warm(engines: &[Engine]) {
    for &e in engines {
        std::thread::spawn(move || {
            if e.open().is_ok() {
                let _ = e.run(vec![0.0; 2 * SEGMENT]); // thread pool, allocator
            }
        });
    }
}

/// The engines a separation uses: the GPU (out of this build, so only the reason is remembered) and the CPU with
/// [`super::threads`] threads.
pub fn engines(multithread: bool) -> [Engine; 2] {
    [Engine::Gpu, Engine::Cpu(super::threads(multithread))]
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::Cell;

    #[test]
    fn a_gpu_that_will_not_open_falls_back_once_and_is_never_asked_again() {
        let (mut gpu, opens, logs) = (Slot::<u32>::Untried, Cell::new(0), Cell::new(0));
        let mut ask = |allowed| {
            gpu.ensure(
                allowed,
                || {
                    opens.set(opens.get() + 1);
                    Err("no Vulkan device".into())
                },
                |why| {
                    assert_eq!(why, "no Vulkan device");
                    logs.set(logs.get() + 1);
                },
            )
            .map(|n| *n)
        };
        assert_eq!(ask(Ok(())), Err("no Vulkan device".into()));
        assert_eq!(ask(Ok(())), Err("no Vulkan device".into()));
        assert_eq!((opens.get(), logs.get()), (1, 1), "tried once, logged once");
    }

    #[test]
    fn a_gpu_that_opens_is_kept_and_a_disabled_one_is_never_opened() {
        let (mut gpu, opens) = (Slot::Untried, Cell::new(0));
        let open = || {
            opens.set(opens.get() + 1);
            Ok(7u32)
        };
        assert_eq!(
            gpu.ensure(Ok(()), open, |_| panic!("works")).map(|n| *n),
            Ok(7)
        );
        assert_eq!(
            gpu.ensure(Ok(()), || panic!("opened twice"), |_| panic!("works"))
                .map(|n| *n),
            Ok(7)
        );
        assert_eq!(opens.get(), 1);

        let mut off = Slot::<u32>::Untried;
        let logged = Cell::new(false);
        let why = off
            .ensure(
                Err("disabled by OPENFADER_STEMS_GPU=0"),
                || panic!("never opened"),
                |_| logged.set(true),
            )
            .map(|n| *n);
        assert_eq!(why, Err("disabled by OPENFADER_STEMS_GPU=0".into()));
        assert!(logged.get(), "the reason is logged, not silent");
    }
}
