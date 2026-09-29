//! rs485-echo：阶段 2 验收固件——USART1 (RS485_1) 缓冲回显 + DE 方向控制
//!
//! 接线事实（0x55 码型扫描实测定案）：
//! - USART1 TX=PD5 / RX=PD6（AF7）——原理图网络名 UART4_TX/RX 系笔误
//! - RS485_DIR1 = PD4：低=发送（DE 使能），高=接收——板级反相接法
//! - 收发器 CS48520S，PC 侧 /dev/ttyUSB1 (CH340)，115200-N8-1
//!
//! 吞吐设计：硬件 RX 缓冲仅 1 字节深，逐字节"收-发-等TC"循环撑不住
//! 连续字节流（溢出丢字节，首版实测只回出乱码）。改为 64 字节缓冲 +
//! 空闲判定（~1ms 无新字节即视为一帧结束）成批回显，DE 在整批期间
//! 恒为发送态，只在帧边界翻转。
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_gd32::{Pin, Port, Rcc, Usart};
use panic_halt as _;

const PCLK1_HZ: u32 = 16_000_000;
const BAUD: u32 = 115_200;
const CHUNK: usize = 256;

/// ~50us 忙等（16MHz，800 次 spin_loop 约 50us——实测校准：太小会导致
/// USB 转发抖动间隙误判"帧结束"，板子在 PC 尚未发完时回显 -> 总线碰撞）
fn delay_50us() {
    for _ in 0..800 {
        core::hint::spin_loop();
    }
}

/// RS485 总线换向保护窗（约 4 字节时间 @115200）：
/// RS485_1 是隔离接口（原理图 ISO），PD4->DIR 与 TX->DI 均经光耦，
/// 传播延迟 us 级。DE 翻转后立即收发会吃掉首字节起始位/截断尾字节
/// 停止位（实测症状：首字节 0x00 损坏为 0xa0、次字节丢失）。
/// 换向窗 = 光耦传播 + 收发器使能时间 + 裕量。
fn turnaround() {
    for _ in 0..6400 {
        core::hint::spin_loop();
    }
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::D);
    rcc.enable_usart1();

    // PD5=TX / PD6=RX，AF7（USART1）
    let _tx = Pin::alternate(&p.gpiod, 5, 7);
    let _rx = Pin::alternate(&p.gpiod, 6, 7);

    // PD4 = RS485_DIR1：低=发送（板级反相，实测定案），空闲=接收态
    let mut de = Pin::output(&p.gpiod, 4);
    de.set_high();

    let uart = Usart::new(&p.usart1);
    uart.enable(PCLK1_HZ, BAUD);

    // 上电 banner（含换向保护窗）
    de.set_low();
    turnaround();
    uart.write(b"rs485-echo ready\r\n");
    uart.flush();
    turnaround();
    de.set_high();

    let mut buf = [0u8; CHUNK];
    let mut n = 0usize;
    let mut idle_ticks = 0u32;

    loop {
        match uart.read_byte() {
            Some(b) => {
                buf[n] = b;
                n += 1;
                idle_ticks = 0;
                if n == CHUNK {
                    // 缓冲满：整批回显（含换向保护窗）
                    de.set_low();
                    turnaround();
                    uart.write(&buf[..n]);
                    turnaround();
                    de.set_high();
                    n = 0;
                    idle_ticks = 0;
                }
            }
            None => {
                if n > 0 {
                    idle_ticks += 1;
                    // ~5ms 无新字节：帧结束（阈值须大于 USB 转发抖动间隙），
                    // 整批回显（含换向保护窗）
                    if idle_ticks >= 100 {
                        de.set_low();
                        turnaround();
                        uart.write(&buf[..n]);
                        turnaround();
                        de.set_high();
                        n = 0;
                        idle_ticks = 0;
                    }
                }
                delay_50us();
            }
        }
    }
}
