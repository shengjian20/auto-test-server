//! control-server 主变体（链接 @0x08008000 应用区，bootloader v2 引导；
//! OTA 槽位镜像的缺省出厂形态）。业务逻辑见 lib 的 app 模块。
#![no_std]
#![no_main]

use control_server::app;
use embassy_executor::Spawner;
use panic_halt as _;

#[embassy_executor::main]
async fn main(_sp: Spawner) {
    app::run("control-server v4 ready\r\n").await
}
