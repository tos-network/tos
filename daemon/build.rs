// Build script - allowed to use expect() per CLAUDE.md (Startup/Init code path)
#![allow(clippy::expect_used)]

fn main() {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .file_descriptor_set_path(out_dir.join("a2a_descriptor.bin"))
        .compile_protos(&["../proto/a2a.proto"], &["../proto"])
        .expect("Failed to compile a2a proto");
}
