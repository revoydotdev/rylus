use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    // Entry points that esbuild should bundle. Keep these in sync with the
    // `build` script in the top-level package.json.
    let bundles: &[(&str, &str)] = &[
        ("ts/lib.ts", "www/static/lib.js"),
        ("ts/sw.ts", "www/static/sw.js"),
        ("ts/utils.ts", "/dev/null"), // track utils too — lib.ts imports it.
    ];

    // Rerun if any TS source or generated JS changes.
    for (ts, js) in bundles {
        let ts_path = workspace_root.join(ts);
        println!("cargo:rerun-if-changed={}", ts_path.display());
        if *js != "/dev/null" {
            println!(
                "cargo:rerun-if-changed={}",
                workspace_root.join(js).display()
            );
        }
    }

    let needs_update = |ts: &Path, js: &Path| -> bool {
        (|| -> Result<bool, Box<dyn std::error::Error>> {
            // If any TS file in the bundle is newer than the output, rebuild.
            let ts_mtime = ts.metadata()?.modified()?;
            let utils_mtime = workspace_root
                .join("ts/utils.ts")
                .metadata()
                .and_then(|m| m.modified())
                .ok();
            let js_mtime = js.metadata()?.modified()?;
            if ts_mtime > js_mtime {
                return Ok(true);
            }
            if let Some(u) = utils_mtime {
                if u > js_mtime {
                    return Ok(true);
                }
            }
            Ok(false)
        })()
        .unwrap_or(true)
    };

    for (ts, js) in bundles {
        if *js == "/dev/null" {
            continue;
        }
        let ts_path: PathBuf = workspace_root.join(ts);
        let js_path: PathBuf = workspace_root.join(js);
        if !needs_update(&ts_path, &js_path) {
            continue;
        }

        let status = Command::new("npx")
            .args([
                "esbuild",
                "--bundle",
                "--target=es2020",
                &format!("--outfile={}", js_path.display()),
                &format!("{}", ts_path.display()),
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
