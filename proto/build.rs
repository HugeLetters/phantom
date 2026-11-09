//! Fixes `BUILD_ID` at compile time: a hash of everything the workspace is built from, so two
//! builds share an id exactly when their sources are the same, committed or not.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

const CRATES: [&str; 5] = ["proto", "client", "service", "cli", "gui"];

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = vec![root.join("Cargo.toml"), root.join("Cargo.lock")];
    for name in CRATES {
        collect(&root.join(name), &mut files);
    }
    files.sort();

    let mut hasher = DefaultHasher::new();
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
        file.strip_prefix(root).unwrap().hash(&mut hasher);
        std::fs::read(file).unwrap_or_default().hash(&mut hasher);
    }
    println!("cargo:rustc-env=PHANTOM_BUILD_ID={:016x}", hasher.finish());
}

/// Every file under `dir` that goes into a build: not `target/` and not hidden.
fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect(&path, files);
        } else {
            files.push(path);
        }
    }
}
