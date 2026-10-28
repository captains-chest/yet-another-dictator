# dictator-engines

The only crate that touches model runtimes (ADR 0008, 0011). Every engine loads from a local,
verified model folder and never downloads models (`scripts/fetch-model.{ps1,sh}` do, for
development).

| Model | Runtime | Crate (exact pin) | Linking |
| --- | --- | --- | --- |
| `parakeet-tdt-0.6b-v2-int8` | sherpa-onnx + ONNX Runtime, CPU | `sherpa-onnx` 2.0.1 (`static`) | prebuilt static libs |
| `whisper-large-v3-turbo-q5_0` | whisper.cpp | `whisper-rs` 0.16.0 | built from vendored source, static, own ggml |
| `s1-mini` (cleanup) | llama.cpp | `llama-cpp-2` 0.1.160 (`dynamic-link`) | built from vendored source, **shared**, own ggml |

Feature `vulkan` puts whisper.cpp and all of S1-mini's layers on the GPU; without it everything
runs on the CPU. Building needs CMake and a C++ toolchain (MSVC on Windows; the Vulkan SDK with
`vulkan`).

## Two ggml copies in one process

whisper.cpp and llama.cpp each vendor their own ggml, and linking both statically fails with
duplicate symbols. llama.cpp is therefore built as shared libraries (`dynamic-link`), which bind
their own ggml, while whisper.cpp's ggml stays inside the executable. `tests/end_to_end.rs` links
all three runtimes into one binary and runs Whisper and S1-mini in one process.

On Windows, ship these next to `Dictator.exe`; the build copies them into `target/<profile>/`:
`llama.dll`, `llama-common.dll`, `ggml.dll`, `ggml-base.dll`, `ggml-cpu.dll`, plus `ggml-vulkan.dll`
with `vulkan`. (On macOS the same set is `lib*.0.dylib`, found through `@rpath`.) Linux would
need care: ELF's flat namespace can bind llama's ggml calls to the executable's copy.

## sherpa-onnx native libraries

`sherpa-onnx-sys` never builds from source. Its build script downloads
`sherpa-onnx-v2.0.1-<platform>-static[-MT-Release]-lib.tar.bz2` from the k2-fsa GitHub release,
**without verifying it**, and caches it in `target/sherpa-onnx-prebuilt/`. For release builds,
fetch and verify the archive first and point the build at it:

```powershell
$env:SHERPA_ONNX_ARCHIVE_DIR = pwsh crates/dictator-engines/scripts/fetch-sherpa-onnx.ps1 win-x64
```

The Windows static archives are built with the static CRT (`/MT`). Rust's MSVC targets default to
`/MD`, so a Windows build needs `-C target-feature=+crt-static` (plus `LLAMA_STATIC_CRT=1` and
`CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded` for the CMake builds; `.cargo/config.toml` sets all three), or a
switch to sherpa-onnx's `shared` feature, which ships `sherpa-onnx-c-api.dll` and `onnxruntime.dll`.

## Platforms

| Crate | win-x64 | win-arm64 |
| --- | --- | --- |
| sherpa-onnx | prebuilt (static and shared) | prebuilt (static and shared) |
| whisper-rs | source build (MSVC + CMake) | source build; ggml has NEON paths |
| llama-cpp-2 | source build | source build |

Vulkan on win-arm64 depends on the GPU driver (Qualcomm Adreno ships Vulkan 1.3).

## Choices that differ from macOS

- **Parakeet**: FluidAudio windows long audio itself; sherpa-onnx decodes whatever it is given, so
  audio over 60 s is cut at pauses (`long_audio_splitter`) into pieces of at most 60 s. That bounds
  memory and lets a stop take effect between pieces. Threads: min(4, cores).
- **Whisper**: WhisperKit's defaults, spelled out: greedy at temperature 0, falling back in steps
  of 0.2 (`best_of` 5), log-prob threshold −1, no-speech threshold 0.6. whisper.cpp has no
  compression-ratio check during decoding; its entropy threshold (2.4) stands in, and segments
  with a zlib ratio over 2.4 are dropped afterwards. The abort callback reads the `StopSignal`.
- **S1-mini**: as on macOS (greedy, special tokens off for the transcript, raw text when the answer
  hits its token limit). One worker thread owns the model, which serialises cleanups.
