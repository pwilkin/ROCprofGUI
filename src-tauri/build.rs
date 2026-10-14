fn main() {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .to_owned();
    let output =
        std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("mcp-analysis.cjs");
    let status = std::process::Command::new("node")
        .current_dir(&root)
        .arg("scripts/build-mcp-analysis.mjs")
        .arg(output)
        .status()
        .expect("Node.js is required to bundle shared MCP analysis");
    assert!(status.success(), "MCP analysis bundling failed");
    println!("cargo:rerun-if-changed=../scripts");
    println!("cargo:rerun-if-changed=../src/mcpAnalysisProtocol.json");
    println!("cargo:rerun-if-changed=../scripts/build-mcp-analysis.mjs");
    println!("cargo:rerun-if-changed=../src");
    tauri_build::build();
}
