use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let frontend_dir = Path::new(&manifest_dir).join("frontend");
    let build_dir = frontend_dir.join("build");
    let index_html = build_dir.join("index.html");

    if !index_html.exists() {
        println!("cargo:warning=Frontend build not found, building frontend...");

        let npm = if cfg!(target_os = "windows") {
            "npm.cmd"
        } else {
            "npm"
        };

        let result = Command::new(npm)
            .arg("run")
            .arg("build")
            .current_dir(&frontend_dir)
            .status();

        match result {
            Ok(status) if status.success() => {
                println!("cargo:warning=Frontend built successfully");
            }
            Ok(status) => {
                panic!("Frontend build failed with status: {}", status);
            }
            Err(e) => {
                panic!("Failed to run npm build: {}. Is Node.js installed?", e);
            }
        }
    }

    println!("cargo:rerun-if-changed=frontend/build");
}
