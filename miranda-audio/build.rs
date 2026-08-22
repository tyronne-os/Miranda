//! WO-2 T6 build script — locates and links this project's existing
//! `parakeet.cpp` native build for the ASR FFI binding.
//!
//! Design decision: this script **auto-detects** parakeet.cpp and does not
//! fail the build when it is absent. Instead it emits
//! `cargo:rustc-cfg=parakeet_available`, and the FFI module is gated on
//! that cfg. Rationale: parakeet.cpp lives outside this repository at an
//! absolute path (it is a local/bare-metal Pipeline 2 dependency, per
//! `.kiro/specs/wo2-acoustic-ingress-routing/tasks.md`'s headless-hardware
//! note). Hard-failing would break `cargo build` at the workspace root on
//! any machine without it — including the WO-1 regression check this
//! project runs constantly — for a dependency that Pipeline 1 does not
//! use at all. Auto-detection keeps the workspace green everywhere while
//! still building and linking the real thing on a machine that has it.
//!
//! Override the search path with `PARAKEET_DIR=/path/to/parakeet.cpp`.

use std::path::PathBuf;

/// Default location of this project's existing parakeet.cpp build.
/// Verified present on the current build machine; overridable via
/// `PARAKEET_DIR` so this is not a hard-coded single-machine assumption.
const DEFAULT_PARAKEET_DIR: &str = "/home/hunt/Applications/parakeet.cpp";

fn main() {
    // Tell Cargo about the custom cfg so newer toolchains don't emit an
    // "unexpected cfg" warning (required since Rust 1.80's check-cfg).
    println!("cargo::rustc-check-cfg=cfg(parakeet_available)");

    println!("cargo:rerun-if-env-changed=PARAKEET_DIR");

    let dir = std::env::var("PARAKEET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_PARAKEET_DIR));

    let lib_dir = dir.join("build");
    let ggml_dir = lib_dir.join("third_party/ggml/src");
    let static_lib = lib_dir.join("libparakeet.a");

    // Every piece must be present. A partial build (e.g. libparakeet.a but
    // no ggml shared objects) would link but fail at runtime with an
    // unresolved-symbol error, which is a worse failure mode than simply
    // not building the FFI at all — so all four are checked together.
    let ggml_libs = ["libggml.so", "libggml-cpu.so", "libggml-base.so"];
    let all_present = static_lib.is_file()
        && ggml_libs.iter().all(|l| ggml_dir.join(l).exists());

    if !all_present {
        println!(
            "cargo:warning=parakeet.cpp not found at {} — miranda-audio's ASR FFI \
             (WO-2 T6) will not be compiled. Pipeline 1 does not need it; set \
             PARAKEET_DIR to enable Pipeline 2's local ASR.",
            dir.display()
        );
        return;
    }

    println!("cargo:rerun-if-changed={}", static_lib.display());

    // Static lib first, then the ggml shared objects it depends on.
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=parakeet");

    println!("cargo:rustc-link-search=native={}", ggml_dir.display());
    for lib in &ggml_libs {
        // Strip "lib" prefix and ".so" suffix for the -l form.
        let name = lib.trim_start_matches("lib").trim_end_matches(".so");
        println!("cargo:rustc-link-lib=dylib={name}");
    }

    // ggml is loaded at runtime from its build directory, so the rpath must
    // be baked in — otherwise the test binary links fine but dies at
    // startup with "libggml.so.0: cannot open shared object file". This
    // mirrors exactly what parakeet.cpp's own CMake link line does (see
    // build/examples/cli/CMakeFiles/parakeet-cli.dir/link.txt).
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", ggml_dir.display());

    // libparakeet.a is C++; the C++ runtime must be linked explicitly
    // because Rust's default link line is C-only.
    println!("cargo:rustc-link-lib=dylib=stdc++");

    // NOTE on ISA (per the `llamacpp-huggingface-expert` skill): this build
    // machine is an Intel Celeron N4500 — verified `sse4_2` only, no AVX,
    // no AVX2, no FMA. No `-march`/`-mavx2` flags are emitted here at all,
    // deliberately: we link a *prebuilt* libparakeet.a that was already
    // compiled from source for this host's real ISA, so there is no C++
    // recompilation happening in this script that could reintroduce the
    // SIGILL (exit 132) illegal-instruction crash that a generic
    // AVX2-assuming binary causes on this CPU.
    println!("cargo:rustc-cfg=parakeet_available");
}
