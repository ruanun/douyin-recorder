fn main() {
    // Compile the minimal Douyin webcast protobuf definitions used for
    // danmaku recording. Requires protoc (protobuf-compiler) on PATH.
    prost_build::Config::new()
        .compile_protos(&["proto/douyin_webcast.proto"], &["proto"])
        .expect("failed to compile douyin_webcast.proto");
    println!("cargo:rerun-if-changed=proto/douyin_webcast.proto");
}
