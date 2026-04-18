use std::fs;
use std::path::{Path, PathBuf};

const MAX_RUST_SOURCE_LINES: usize = 3000;
const IGNORE_DIRS: &[&str] = &["target", ".git"];

fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|err| panic!("read_dir {}: {err}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|err| panic!("read_dir entry {}: {err}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            if path.components().any(|component| {
                IGNORE_DIRS.contains(&component.as_os_str().to_string_lossy().as_ref())
            }) {
                continue;
            }
            visit(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn count_lines(path: &Path) -> usize {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
        .lines()
        .count()
}

#[test]
fn rust_source_files_stay_within_line_budget() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    visit(&repo_root.join("src"), &mut files);
    visit(&repo_root.join("tests"), &mut files);
    files.sort();

    let over_budget: Vec<String> = files
        .into_iter()
        .filter_map(|path| {
            let lines = count_lines(&path);
            (lines > MAX_RUST_SOURCE_LINES).then(|| {
                let relative = path.strip_prefix(&repo_root).unwrap_or(&path);
                format!("{}: {} lines", relative.display(), lines)
            })
        })
        .collect();

    assert!(
        over_budget.is_empty(),
        "Rust source file line budget exceeded (>{MAX_RUST_SOURCE_LINES} lines):\n{}",
        over_budget.join("\n")
    );
}
