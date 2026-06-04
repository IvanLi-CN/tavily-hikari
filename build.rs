use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=web/dist");
    println!("cargo:rustc-check-cfg=cfg(web_assets_embedded)");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let generated_path = out_dir.join("embedded_web_assets.rs");
    let dist_dir = Path::new("web/dist");

    if dist_dir.is_dir() {
        println!("cargo:rustc-cfg=web_assets_embedded");

        let assets_out_dir = out_dir.join("web-assets");
        if assets_out_dir.exists() {
            fs::remove_dir_all(&assets_out_dir)?;
        }
        fs::create_dir_all(&assets_out_dir)?;

        let mut files = Vec::new();
        let mut dirs = Vec::new();
        collect_files(dist_dir, &mut files, &mut dirs)?;
        files.sort();
        dirs.sort();

        for dir in &dirs {
            println!("cargo:rerun-if-changed={}", dir.display());
        }
        for file in &files {
            println!("cargo:rerun-if-changed={}", file.display());

            let rel = file.strip_prefix(dist_dir).expect("dist relative path");
            let dest = assets_out_dir.join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(file, dest)?;
        }

        let mut source =
            String::from("pub fn get(path: &str) -> Option<&'static [u8]> {\n    match path {\n");
        for file in &files {
            let rel = file.strip_prefix(dist_dir).expect("dist relative path");
            let rel = rel.to_string_lossy().replace('\\', "/");
            let rel_literal = format!("{rel:?}");
            source.push_str(&format!(
                "        {rel_literal} => Some(include_bytes!(concat!(env!(\"OUT_DIR\"), \"/web-assets/\", {rel_literal}))),\n"
            ));
        }
        source.push_str("        _ => None,\n    }\n}\n");
        fs::write(generated_path, source)?;
    } else {
        fs::write(
            generated_path,
            "pub fn get(_path: &str) -> Option<&'static [u8]> {\n    None\n}\n",
        )?;
    }

    Ok(())
}

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>, dirs: &mut Vec<PathBuf>) -> io::Result<()> {
    dirs.push(dir.to_path_buf());
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files, dirs)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}
