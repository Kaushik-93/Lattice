//! Generates the gateway ⇄ AI gRPC code from packages/proto. A vendored protoc keeps the
//! build free of system dependencies.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../packages/proto");
    let protos = ["lattice/ai/v1/ai.proto", "lattice/ai/v1/tools.proto"].map(|p| root.join(p));

    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    tonic_prost_build::configure().compile_with_config(
        config,
        &protos,
        std::slice::from_ref(&root),
    )?;

    println!("cargo:rerun-if-changed={}", root.display());
    Ok(())
}
