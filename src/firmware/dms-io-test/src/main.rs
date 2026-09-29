//! dms-io-test：阶段 2h 验收固件——DMS IO×18（OUT 走位 + IN/CTRL 状态流）
//!
//! 引脚映射（原理图引脚页文本层推导，COL 定位已按 PB/PE 左右列核对；
//! 电气回读验收需外部 jumper/meter，故固件提供可观测走位模式 + 状态流）：
//! - DMS_OUT1-8 = PE9/PE10/PE11/PE12/PE13/PE14/PE15 + PB10（AQY282S SSR 驱动）
//! - DMS_IN1-8  = PD10/PD11/PD12/PD13/PD14/PD15 + PC6/PC7（TLP290 光耦输入）
//! - DMS_CTRL1-2 = PB3/PB4
//! - Console = UART6 PE7/PE8（AF8）115200
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_gd32::{Pin, Port, Rcc, Uart};
use panic_halt as _;

const PCLK1_HZ: u32 = 16_000_000;

fn delay_ms(ms: u32) {
    for _ in 0..ms {
        for _ in 0..4000 {
            core::hint::spin_loop();
        }
    }
}

fn put_str(uart: &Uart, s: &str) {
    uart.write(s.as_bytes());
}

fn put_hex8(uart: &Uart, v: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    uart.write_byte(HEX[(v >> 4) as usize]);
    uart.write_byte(HEX[(v & 0xF) as usize]);
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::B);
    rcc.enable_gpio_port(Port::C);
    rcc.enable_gpio_port(Port::D);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();

    // UART6 引脚：PE7=TX / PE8=RX（AF8）——缺失会导致 console 静默
    // （同 flash-identify 首版疏漏：外设配置正确但引脚停在复位态 AF0）
    let _utx = Pin::alternate(&p.gpioe, 7, 8);
    let _urx = Pin::alternate(&p.gpioe, 8, 8);

    // OUT1-7 = PE9..PE15，OUT8 = PB10
    let mut outs_e: [Pin; 7] = [
        Pin::output(&p.gpioe, 9),
        Pin::output(&p.gpioe, 10),
        Pin::output(&p.gpioe, 11),
        Pin::output(&p.gpioe, 12),
        Pin::output(&p.gpioe, 13),
        Pin::output(&p.gpioe, 14),
        Pin::output(&p.gpioe, 15),
    ];
    let mut out8 = Pin::output(&p.gpiob, 10);
    // CTRL1-2 = PB3/PB4
    let mut ctrl1 = Pin::output(&p.gpiob, 3);
    let mut ctrl2 = Pin::output(&p.gpiob, 4);
    // IN1-6 = PD10..PD15，IN7-8 = PC6/PC7
    let ins_d: [Pin; 6] = [
        Pin::input(&p.gpiod, 10),
        Pin::input(&p.gpiod, 11),
        Pin::input(&p.gpiod, 12),
        Pin::input(&p.gpiod, 13),
        Pin::input(&p.gpiod, 14),
        Pin::input(&p.gpiod, 15),
    ];
    let ins_c: [Pin; 2] = [Pin::input(&p.gpioc, 6), Pin::input(&p.gpioc, 7)];

    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK1_HZ, 115_200);
    put_str(&uart, "dms-io-test: OUT walk PE9-15+PB10, IN PD10-15+PC6/7, CTRL PB3/4\r\n");

    let mut step: u8 = 0;
    loop {
        // 走位：OUTx 置高 500ms（SSR 导通可观测）
        outs_e[step as usize % 7].set_high();
        if step == 7 {
            out8.set_high();
        }
        delay_ms(500);
        outs_e[step as usize % 7].set_low();
        if step == 7 {
            out8.set_low();
        }
        step = (step + 1) % 8;

        // 输入状态流（每 8 步打一次 = ~4s）
        if step == 0 {
            let mut in_byte = 0u8;
            for (i, pin) in ins_d.iter().enumerate() {
                if pin.input_level() {
                    in_byte |= 1 << i;
                }
            }
            for (i, pin) in ins_c.iter().enumerate() {
                if pin.input_level() {
                    in_byte |= 1 << (6 + i);
                }
            }
            put_str(&uart, "IN=0x");
            put_hex8(&uart, in_byte);
            put_str(&uart, " CTRL=");
            let c = if ctrl1.output_level() { "1" } else { "0" };
            put_str(&uart, c);
            put_str(&uart, "\r\n");
            // CTRL 翻转演示
            if ctrl1.output_level() {
                ctrl1.set_low();
                ctrl2.set_high();
            } else {
                ctrl1.set_high();
                ctrl2.set_low();
            }
        }
    }
}
