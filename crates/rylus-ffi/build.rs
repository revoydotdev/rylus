use std::env;
use std::path::Path;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    // Workspace root is two levels up from this crate
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let lib_dir = workspace_root.join("lib");

    // error.h and log.h are now header-only (static inline functions).
    // No separate .c files to compile — they are included directly by xcapture.c/xhelper.c.
    println!(
        "cargo:rerun-if-changed={}",
        lib_dir.join("error.h").display()
    );
    println!("cargo:rerun-if-changed={}", lib_dir.join("log.h").display());

    // FFmpeg encoding has been moved to the rylus-encode crate (using ffmpeg-sys-next).
    // encode_video.c is no longer compiled here.

    if target_os == "linux" {
        // X11 C code is only compiled when the x11 feature is enabled
        if env::var("CARGO_FEATURE_X11").is_ok() {
            linux_x11(&lib_dir);
        }
    }
}

fn linux_x11(lib_dir: &Path) {
    println!(
        "cargo:rerun-if-changed={}",
        lib_dir.join("linux/xcapture.c").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        lib_dir.join("linux/xhelper.c").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        lib_dir.join("linux/xhelper.h").display()
    );

    cc::Build::new()
        .file(lib_dir.join("linux/xcapture.c"))
        .file(lib_dir.join("linux/xhelper.c"))
        .compile("linux");

    println!("cargo:rustc-link-lib=X11");
    println!("cargo:rustc-link-lib=Xext");
    println!("cargo:rustc-link-lib=Xrandr");
    println!("cargo:rustc-link-lib=Xfixes");
    println!("cargo:rustc-link-lib=Xcomposite");
    println!("cargo:rustc-link-lib=Xi");
}
