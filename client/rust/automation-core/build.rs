//! Embed only the public helper sources and pinned dependency locks.
use std::{env, fs, path::Path};
fn collect(root: &Path, path: &Path, files: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(path).expect("Runner source directory") {
        let path = entry.expect("Runner entry").path();
        if path.is_dir() {
            if path.file_name().unwrap() != "__pycache__" {
                collect(root, &path, files);
            }
        } else if matches!(
            path.extension().and_then(|v| v.to_str()),
            Some("py" | "json")
        ) {
            files.push((
                path.strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/"),
                // These are installed on Linux regardless of the desktop build
                // platform. Git checkout CRLF must not change runtime identity.
                fs::read_to_string(path).unwrap().replace("\r\n", "\n"),
            ));
        }
    }
}
fn main() {
    let root = Path::new("../../../runner");
    println!("cargo:rerun-if-changed={}", root.join("src").display());
    let mut files = vec![];
    collect(root, &root.join("src/consolecrypt_runner"), &mut files);
    for name in [
        "requirements-linux-x86_64-py312.lock",
        "requirements-linux-x86_64-py314.lock",
        "requirements-linux-aarch64-py312.lock",
        "requirements-linux-aarch64-py313.lock",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(name).display());
        files.push((name.into(), fs::read_to_string(root.join(name)).unwrap()));
    }
    files.sort();
    let source = format!("const SETUP_FILES: &[(&str, &str)] = &{:?};", files);
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("runner_package.rs"),
        source,
    )
    .unwrap();
}
