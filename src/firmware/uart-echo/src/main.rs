//! uart-echo：阶段 2 第一个验收固件——UART6 (PC_RS232_1) 轮询回显
//!
//! 接线事实（carrier-box 实跑配置 + target.md）：UART6 TX=PE7 / RX=PE8 (AF8)，
//! 对应 PC 侧 /dev/ttyUSB0 (ATEN 串口桥)。115200-N8-1。
//! 验收：PC 发送任意字节流，回显比对一致。
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_gd32::{Pin, Port, Rcc, Uart};
use panic_halt as _;

/// HSI 16MHz，APB1 复位分频 = 1
const PCLK1_HZ: u32 = 16_000_000;
const BAUD: u32 = 115_200;

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();

    // PE7=TX / PE8=RX，AF8（UART6/7）
    let _tx = Pin::alternate(&p.gpioe, 7, 8);
    let _rx = Pin::alternate(&p.gpioe, 8, 8);

    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK1_HZ, BAUD);

    uart.write(b"uart-echo ready\r\n");

    loop {
        if let Some(b) = uart.read_byte() {
            uart.write_byte(b);
        }
    }
}
