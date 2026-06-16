use std::path::{Path, PathBuf};

pub fn resolve_support_binary(env_var: &str, compile_time_path: &str) -> PathBuf {
    if let Some(path) = std::env::var_os(env_var) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return path;
        }
        panic!(
            "support binary from env {env_var} does not exist: {}",
            path.display()
        );
    }

    let compile_time = PathBuf::from(compile_time_path);
    if compile_time.is_file() {
        return compile_time;
    }

    let file_name = Path::new(compile_time_path)
        .file_name()
        .expect("compile-time binary path should have a file name");
    let current_exe = std::env::current_exe().expect("resolve current test executable");
    if let Some(sibling_path) = current_exe.parent().map(|parent| parent.join(file_name))
        && sibling_path.is_file()
    {
        return sibling_path;
    }

    panic!(
        "unable to resolve support binary {env_var}; checked env override, compile-time path, and sibling path for {}",
        file_name.to_string_lossy()
    );
}
