#[cfg(feature = "ppu-sim")]
#[cfg(target_os = "windows")]
fn build_ppu_sim() {
    use std::path::PathBuf;

    let src = vec![
        "breaknes/Common/BaseLogicLib/BaseLogic.cpp",
        "breaknes/Common/BaseLogicLib/pch.cpp",
        "breaknes/BreaksPPU/PPUSim/bgcol.cpp",
        "breaknes/BreaksPPU/PPUSim/cram.cpp",
        "breaknes/BreaksPPU/PPUSim/dataread.cpp",
        "breaknes/BreaksPPU/PPUSim/debug.cpp",
        "breaknes/BreaksPPU/PPUSim/fifo.cpp",
        "breaknes/BreaksPPU/PPUSim/fsm.cpp",
        "breaknes/BreaksPPU/PPUSim/hv.cpp",
        "breaknes/BreaksPPU/PPUSim/hv_decoder.cpp",
        "breaknes/BreaksPPU/PPUSim/mux.cpp",
        "breaknes/BreaksPPU/PPUSim/oam.cpp",
        "breaknes/BreaksPPU/PPUSim/par.cpp",
        "breaknes/BreaksPPU/PPUSim/patgen.cpp",
        "breaknes/BreaksPPU/PPUSim/pch.cpp",
        "breaknes/BreaksPPU/PPUSim/pclk.cpp",
        "breaknes/BreaksPPU/PPUSim/ppu.cpp",
        "breaknes/BreaksPPU/PPUSim/regs.cpp",
        "breaknes/BreaksPPU/PPUSim/scroll_regs.cpp",
        "breaknes/BreaksPPU/PPUSim/sprite_eval.cpp",
        "breaknes/BreaksPPU/PPUSim/video_out.cpp",
        "breaknes/BreaksPPU/PPUSim/vram_ctrl.cpp",
        "breaknes-bindings/ppusim-bindings.cpp",
    ];

    //let cwd = std::env::current_dir().unwrap();
    //println!("current dir = {}", std::env::current_dir().unwrap().display());
    let mut build = cc::Build::new();
    build.include("breaknes/Common/BaseLogicLib");
    build.include("breaknes/BreaksPPU/PPUSim");
    build.include("breaknes-bindings");
    for path in src {
        //let abs = cwd.join(path);
        //println!("Adding {}", abs.display());
        //let buf = PathBuf::from(path);
        //let abs = std::fs::canonicalize(&buf).unwrap();

        build.file(path);

        println!("cargo:rerun-if-changed={path}");
    }

    build.compile("libppusim.a");

    let bindings = bindgen::Builder::default()
        .derive_debug(true)
        .derive_default(true)
        // The input header we would like to generate
        // bindings for.
        .header("breaknes-bindings/ppusim-bindings.h")
        .clang_arg("-Ibreaknes/Common/BaseLogicLib")
        .clang_arg("-Ibreaknes/BreaksPPU/PPUSim")
        .clang_arg("-Ibreaknes-bindings")
        .clang_arg("-x")
        .clang_arg("c++")
        .clang_arg("-std=c++20")
        .clang_arg("-fms-extensions")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    /*
    cxx_build::bridge("src/ppusim.rs")
        .include("breaknes/Common/BaseLogicLib")
        .include("breaknes/BreaksPPU/PPUSim")
        .include("breaknes-bindings")
        .compile("ppusim-cxx");

    println!("cargo:rerun-if-changed=src/ppusim.rs");
    println!("cargo:rerun-if-changed=breaknes-bindings/ppusim-bindings.h");
    println!("cargo:rerun-if-changed=breaknes-bindings/ppusim-bindings.cpp");
    */
}

fn main() {
    // Note: for now PPUSim is only supported on Windows since it depends on
    // various MSVC extensions
    #[cfg(feature = "ppu-sim")]
    #[cfg(target_os = "windows")]
    build_ppu_sim();
}
