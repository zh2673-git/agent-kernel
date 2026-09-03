//! 编译 `schema/kernel.proto` 为 Rust stub（tonic + prost）。
//! 契约唯一源：仓库根 `schema/kernel.proto`（与 docs/05 §4.2 一致）。

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto = "../../schema/kernel.proto";
    let include = "../../schema";
    println!("cargo:rerun-if-changed={proto}");
    tonic_prost_build::configure()
        .compile_protos(&[proto], &[include])?;
    Ok(())
}
