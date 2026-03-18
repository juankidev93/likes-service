use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=proto/likes.proto");

    let protoc_path = protoc_bin_vendored::protoc_bin_path()
        .expect("failed to resolve vendored protoc binary");
    unsafe {
        std::env::set_var("PROTOC", protoc_path);
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must be set"));

    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("likes_descriptor.bin"))
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/likes.proto"], &["proto"])
        .expect("failed to compile gRPC protobuf definitions");
}
