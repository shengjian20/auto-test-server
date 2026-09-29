//! control-server-ota：OTA 变体（@0x08008000，与主变体同一 app 主体）。
//! 经 PC 写入 W25Q 槽位 -> bootloader v2 校验 CRC -> 搬运到应用区。
#![no_std]
#![no_main]

use control_server::app;
use embassy_executor::Spawner;
use panic_halt as _;

#[embassy_executor::main]
async fn main(_sp: Spawner) {
    app::run("control-server-ota v4 ready (@0x08008000)\r\n").await
}
