//! control-server 主变体（链接 @0x08008000 应用区，bootloader v2 引导；
//! OTA 槽位镜像的缺省出厂形态）。业务逻辑见 lib 的 app 模块。
#![no_std]
#![no_main]

use control_server::app;
use embassy_executor::Spawner;
use linked_list_allocator::LockedHeap;
use panic_halt as _;

// smoltcp alloc feature 经共享依赖进链接图（TCP 变体同 crate），UART
// 变体不分配但链接器仍要求全局分配器符号
static mut HEAP_MEM: [u8; 4096] = [0; 4096];

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// unsafe 依据：单核单点初始化（main 最前）
fn init_heap() {
    unsafe {
        let mem = core::ptr::addr_of_mut!(HEAP_MEM);
        ALLOCATOR.lock().init((*mem).as_mut_ptr(), (*mem).len());
    }
}

#[embassy_executor::main]
async fn main(_sp: Spawner) {
    init_heap();
    app::run("control-server v4 ready\r\n").await
}
