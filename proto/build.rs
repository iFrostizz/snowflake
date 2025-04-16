use std::io::Result;

fn main() -> Result<()> {
    std::env::set_var("OUT_DIR", "src");

    let protos: Vec<String> = std::fs::read_dir(".")?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "proto"))
        .filter_map(|path| {
            path.file_name()
                .and_then(|file_name| file_name.to_str().map(|name| name.to_string()))
        })
        .collect();

    prost_build::compile_protos(&protos, &["./"])?;

    println!("cargo:rerun-if-changed=.");

    Ok(())
}
