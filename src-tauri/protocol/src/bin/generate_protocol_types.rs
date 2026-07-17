fn main() {
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root");
    soia_protocol::export_types(workspace_root.join("src/core-client/generated"))
        .expect("failed to generate protocol TypeScript types");
}
