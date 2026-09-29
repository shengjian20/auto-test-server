//! uart-echo：UART6 (PC_RS232_1) 缓冲回显
//!
//! 硬件 RX 缓冲 1 字节深，PC 连续字节流会导致 ORE 丢字节（实测：
//! 逐字节回显在 256B 连发时 rx=0）——64B 缓冲 + 5ms 空闲判定成批
//! 回显（RS485_1 已验证同款方案）。
//! 接线：UART6 TX=PE7 / RX=PE8（AF8），PC 侧 /dev/ttyUSB0 (ATEN)。
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_gd32::{Pin, Port, Rcc, Uart};
use panic_halt as _;

const PCLK1_HZ: u32 = 16_000_000;
const BAUD: u32 = 115_200;
const CHUNK: usize = 512;

/// ~50us 忙等（16MHz，800 次 spin_loop）
fn delay_50us() {
    for _ in 0..800 {
        core::hint::spin_loop();
    }
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();

    let _tx = Pin::alternate(&p.gpioe, 7, 8);
    let _rx = Pin::alternate(&p.gpioe, 8, 8);

    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK1_HZ, BAUD);

    uart.write(b"uart-echo v2-buffered\r\n");

    let mut buf = [0u8; CHUNK];
    let mut n = 0usize;

    loop {
        // 空闲态：慢速轮询等首字节（50us 步进，低 CPU 占用）
        match uart.read_byte() {
            Some(b) => {
                buf[n] = b;
                n += 1;
                // 首字节到达：切换到快速收集（无延时纯轮询，撑满硬件
                // 1 字节缓冲的间隔——115200 下字节间隔 ~87us，纯轮询
                // 一次 read_byte <1us，余量充足）
                // 快速收集：USB 转发是突发分包（CH340 每包间有 ~1ms 级
                // 间隙），单次 None 不代表帧结束——空转计数超过阈值才确认
                // 空闲（阈值 = USB 包间隙余量，实测 3000 次 spin_loop ≈ 200µs
                // 不足以跨 1ms 包隙，取 60000 次 ≈ 4ms）
                let mut idle = 0u32;
                loop {
                    match uart.read_byte() {
                        Some(nb) => {
                            if n < CHUNK {
                                buf[n] = nb;
                                n += 1;
                            }
                            idle = 0;
                        }
                        None => {
                            idle += 1;
                            if idle >= 60_000 {
                                break; // 真空闲（>4ms 无字节）
                            }
                        }
                    }
                }
                // 回显本批
                uart.write(&buf[..n]);
                n = 0;
            }
            None => delay_50us(),
        }
    }
}
