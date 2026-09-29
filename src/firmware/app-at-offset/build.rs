fn main() {
    // 本目录 memory.x（应用 @0x08008000）经 cargo:rustc-link-search 提供给
    // link.x 的 INCLUDE memory.x；不用 .cargo/config rustflags（根 workspace
    // 配置会叠加，导致 -Tlink.x 双份 -> memory.x 双份 -> FLASH 重复定义）
    println!("cargo:rustc-link-search={}", std::env::var("CARGO_MANIFEST_DIR").unwrap());
    println!("cargo:rerun-if-changed=memory.x");
}
