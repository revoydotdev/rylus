use std::path::Path;
use std::process::Command;

fn main() {
    // TypeScript compilation
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let ts_file = workspace_root.join("ts/lib.ts");
    let js_file = workspace_root.join("www/static/lib.js");

    println!("cargo:rerun-if-changed={}", ts_file.display());

    #[cfg(not(target_os = "windows"))]
    let mut tsc_command = Command::new("tsc");

    #[cfg(target_os = "windows")]
    let mut tsc_command = Command::new("bash");
    #[cfg(target_os = "windows")]
    tsc_command.args(&["-c", "tsc"]);

    let js_needs_update = || -> Result<bool, Box<dyn std::error::Error>> {
        Ok(ts_file.metadata()?.modified()? > js_file.metadata()?.modified()?)
    }()
    .unwrap_or(true);

    if js_needs_update {
        // Run tsc from the workspace root where tsconfig.json lives
        tsc_command.current_dir(workspace_root);
        match tsc_command.status() {
            Err(err) => {
                println!("cargo:warning=Failed to call tsc: {}", err);
                std::process::exit(1);
            }
            Ok(status) => {
                if !status.success() {
                    match status.code() {
                        Some(code) => println!("cargo:warning=tsc failed with exitcode: {}", code),
                        None => println!("cargo:warning=tsc terminated by signal."),
                    };
                    std::process::exit(2);
                }
            }
        }
    }
}
