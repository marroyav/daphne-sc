fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = "../../proto/upstream";
    let rust_proto_root = "../../proto";
    let protos = [
        format!("{proto_root}/daphneV3_high_level_confs.proto"),
        format!("{proto_root}/daphneV3_low_level_confs.proto"),
        format!("{rust_proto_root}/daphne_sc.proto"),
    ];

    for proto in &protos {
        println!("cargo:rerun-if-changed={proto}");
    }

    prost_build::compile_protos(&protos, &[proto_root, rust_proto_root])?;
    Ok(())
}
