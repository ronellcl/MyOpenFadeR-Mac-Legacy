# MyOpenFadeR Mac Legacy — macOS 10.15 Catalina (Intel)

This is a **legacy build of [MyOpenFadeR](https://github.com/ronellcl/MyOpenFadeR)** for **macOS 10.15 Catalina on Intel
(x86_64)**, with STEMS. It is a separate tree, not a fork on GitHub: GitHub does not let one account own both a repository and
a fork of it, so this repository was created by pushing the full history of `ronellcl/MyOpenFadeR` and keeping it as
`upstream`.

- **Supported:** macOS 10.15.7, Intel `x86_64`.
- **Not supported:** 10.13, 10.14, Apple silicon, `universal2`, the Mac App Store. The binary may start on Intel 11–15, but
  nothing there is tested.
- **Licence:** AGPL-3.0, unchanged (`LICENSE`), with the attribution to OpenFadeR (OpenRsG) and MyOpenFadeR (ronellcl).

## What is different from the main build

| Area | Main build | This tree |
|---|---|---|
| ONNX Runtime | 1.31 through `ort` 2.0 | **1.16.3**, through a small C-API layer (`src/stems/model.rs`, `src/stems/ort.rs`) |
| STEMS engine | CPU and WebGPU | **CPU only** (1.16.3 has no WebGPU provider); `Engine::Gpu` stays in the enum as "out of this build" |
| Constant folding | off, via a 1.17+ session option | **on**, because 1.16.3 has no such option (`OPENFADER_STEMS_OPT=0` avoids it) |
| Deployment target | 11.0 | **10.15** (`.cargo/config.toml`, `Info.plist`) |
| Architectures | Linux, Windows, macOS arm64 | **macOS x86_64 only** |
| Update check | `ronellcl/MyOpenFadeR` | `ronellcl/MyOpenFadeR-Mac-Legacy`, with `-catalina.N` build numbers |
| Data folder | `~/Library/Application Support/openfader` | the same one (see the schema warning below) |

`LEGACY.md` (this file) is the list of patches. Everything else is upstream's code.

## Why ONNX Runtime 1.16.3

The macOS x86_64 dylibs of ONNX Runtime declare a minimum macOS version, and 1.16.3 is the newest one that runs on 10.15:

| ONNX Runtime | `minos` | Runs on 10.15 |
|---|---|---|
| 1.16.3 | 10.14 | **yes** |
| 1.17.3 – 1.19.2 | 11.0 | no |
| 1.20.1 – 1.22.0 | 13.3 | no |
| 1.31 | arm64 only | no |

The Rust wrapper for it (`ort` 1.16 and its `ort-sys`) is **yanked from crates.io** and its artifacts are gone, so this tree
calls ONNX Runtime's C API (version 16) directly:

- `ort-sys` 2.0 supplies the **types** only, with linking disabled. Its `OrtApi` is the version-17 struct; the version-16
  struct that 1.16.3 returns is a prefix of it, so every function pointer used keeps its offset (`GetApi(16)` asks for it).
- `libloading` opens the dylib at run time, so **building needs no ONNX Runtime at all** and the link stays free of the CoreML
  symbols that force LTO off in the main build.
- `src/stems/model.rs` keeps its interface (`Model::open`, `Model::run`); `src/stems/ort.rs` is the C-API layer.

## Building

On an Intel Mac with a recent macOS (11–15) and Xcode 16 or 26:

```sh
cargo build --release
python3 .github/fetch_tools.py        # ffmpeg/ffprobe for `tools/`
```

`.cargo/config.toml` pins the deployment target, and `rust-toolchain.toml` the compiler. To check what the binaries claim:

```sh
vtool -show-build target/release/openfader
```

Catalina cannot build this (Xcode 16 needs a newer macOS): build on CI or on a newer Intel Mac and test on 10.15.7.

## Packaging

The release workflow (`.github/workflows/release-macos-catalina.yml`) builds `MyOpenFadeR-<version>-macos-catalina-x86_64.zip`:

- `MyOpenFadeR.app/Contents/MacOS/openfader`, the plugins and `tools/` (ffmpeg, ffprobe);
- `Contents/Frameworks/libonnxruntime.1.16.3.dylib`, found by `model::init_ort` (or `$ORT_DYLIB_PATH`);
- `Contents/Resources/onnxruntime/` with ONNX Runtime's `LICENSE` and `ThirdPartyNotices.txt`;
- `LSMinimumSystemVersion` 10.15, ad-hoc signature (`codesign -s -`, the dylib first, then the app).

No Developer ID and no notarisation: after moving the app to `/Applications`, **right-click > Open** once, or

```sh
xattr -dr com.apple.quarantine /Applications/MyOpenFadeR.app
```

## STEMS: memory and speed

ONNX Runtime 1.16.3 folds the in-graph STFT into 2.2 GB of constants with its basic optimisations, and 1.16 has neither the
`optimization.disable_specified_optimizers` option nor the per-provider arena switch that the main build uses. The build
therefore runs the graph **unoptimized**, which measures the same speed for a third of the memory (one 60 s track, 8-core
machine, ORT 1.16.3, `examples/stems_bench.rs`):

| `OPENFADER_STEMS_OPT` | Graph optimisations | 60 s track | Peak RSS |
|---|---|---|---|
| unset | none | 70 s | **1.2 GB** |
| `1` / `basic` | constant folding | 71 s | 4.3 GB |
| `all` | all levels | 63 s | 4.3 GB |

The same track through the main build (ONNX Runtime 1.31, folding off) takes 70 s and 1.4 GB here, with byte-identical stem
levels: the C-API layer is not the difference, the hardware is. `examples/seg_bench.rs` times a single segment.

The model itself (316 MB) is downloaded to the same cache as the main build and verified by SHA256.

## Data folder

This build uses `~/Library/Application Support/openfader`, **the same as the main build**, so the library, playlists and
preferences are shared. The store refuses a `store.db` written by a *newer* schema: **do not go back to this legacy build
after running a newer MyOpenFadeR on the same Mac** (delete or restore `store.db` if you must).

## Plugins

The catalogue asks for `<plugin>-macos-x86_64.zip`. Themes and controller mappings are architecture-independent and work
unchanged. A native plugin published only for `aarch64` will not load here.

## Syncing with upstream

```sh
git fetch upstream
git merge upstream/main        # resolve, then keep the patches below
```

The patch list, in the order they matter:

1. `Cargo.toml` — `ort-sys` (types only) instead of `ort`; no `stems-gpu` feature.
2. `src/stems/ort.rs` — new: the C-API layer (version 16).
3. `src/stems/model.rs` — `Model::open`/`run` on that layer, `init_ort`, `optimization_level`, no GPU session or folding.
4. `src/main.rs`, `examples/stems_bench.rs` — `init_ort()` before anything uses ONNX Runtime.
5. `src/stems/mod.rs` — `pub mod ort;`.
6. `src/app/about.rs` — legacy update repository, `-catalina.N` ordering.
7. `.cargo/config.toml`, `rust-toolchain.toml`, `Info.plist` (in the workflow), `.github/workflows/`.
8. `README.md` — the notice at the top.
