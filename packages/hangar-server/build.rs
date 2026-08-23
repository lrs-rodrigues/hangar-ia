fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    // The build does not depend on a host-installed protoc, preserving Docker-only validation.
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos_with_config(config, &["proto/hangar/v1/hangar.proto"], &["proto"])?;
    println!("cargo:rerun-if-changed=proto/hangar/v1/hangar.proto");
    Ok(())
}
