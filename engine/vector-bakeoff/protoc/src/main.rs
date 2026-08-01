fn main() -> Result<(), String> {
    let path = protoc_bin_vendored::protoc_bin_path()
        .map_err(|error| format!("resolve vendored protoc: {error}"))?;
    println!("{}", path.display());
    Ok(())
}
