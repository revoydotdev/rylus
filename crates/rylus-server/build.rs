use std::path::Path;
use std::process::Command;

fn main() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let ts_file = workspace_root.join("ts/lib.ts");
    let js_file = workspace_root.join("www/static/lib.js");

    // Rebuild when the TS source or the generated JS changes
    println!("cargo:rerun-if-changed={}", ts_file.display());
    println!("cargo:rerun-if-changed={}", js_file.display());

    let js_needs_update = || -> Result<bool, Box<dyn std::error::Error>> {
        Ok(ts_file.metadata()?.modified()? > js_file.metadata()?.modified()?)
    }()
    .unwrap_or(true);

    if js_needs_update {
        let status = Command::new("npx")
            .args([
                "esbuild",
                "ts/lib.ts",
                "--target=es2020",
                "--outfile=www/static/lib.js",
            ])
            .current_dir(workspace_root)
            .status();

        match status {
            Err(err) => {
                println!("cargo:warning=Failed to call esbuild: {err}");
                std::process::exit(1);
            }
            Ok(s) if !s.success() => {
                println!(
                    "cargo:warning=esbuild failed with exit code: {}",
                    s.code().map_or("signal".to_string(), |c| c.to_string())
                );
                std::process::exit(2);
            }
            Ok(_) => {}
        }
    }
}
