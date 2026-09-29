//! can-loopback：阶段 2e 验收固件——CAN0 内部回环模式收发比对
//!
//! 事实链（carrier-box 实跑 .config）：CAN0_RX=PD0 / CAN0_TX=PD1（AF9），
//! RCU_APB1EN.CAN0EN。回环模式 BT.LCMOD=1（内部 TX->RX 短接，不依赖
//! PD1 物理引脚/收发器），验收 bxCAN 外设与驱动逻辑本身。
//! 结果经 UART6 console（PE7/PE8 AF8 115200）输出，单轮后挂起。
#![no_std]
#![no_main]
#![deny(unsafe_code)]

use embassy_gd32::{Can, Pin, Port, Rcc, Uart};
use panic_halt as _;

const PCLK1_HZ: u32 = 16_000_000;

fn delay_ms(ms: u32) {
    for _ in 0..ms {
        for _ in 0..4000 {
            core::hint::spin_loop();
        }
    }
}

fn put_hex(uart: &Uart, v: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    uart.write_byte(HEX[(v >> 4) as usize]);
    uart.write_byte(HEX[(v & 0xF) as usize]);
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::D);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_can0();
    rcc.enable_uart6();

    // CAN0 引脚：PD0=RX / PD1=TX（AF9）。回环模式下不依赖物理引脚，
    // 但仍配置好，为后续真实总线测试备用
    let _rx = Pin::alternate(&p.gpiod, 0, 9);
    let _tx = Pin::alternate(&p.gpiod, 1, 9);

    // UART6 console
    let _utx = Pin::alternate(&p.gpioe, 7, 8);
    let _urx = Pin::alternate(&p.gpioe, 8, 8);
    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK1_HZ, 115_200);
    uart.write(b"can-loopback\r\n");

    let can = Can::new(&p.can0, &p.can0);
    match can.init_loopback(&embassy_gd32::can::BitTiming::kbps500()) {
        Ok(()) => uart.write(b"can init: OK\r\n"),
        Err(e) => {
            uart.write(b"can init: ");
            uart.write(e.as_bytes());
            uart.write(b"\r\n");
            loop { core::hint::spin_loop(); }
        }
    }

    // 发送 3 帧，每帧立即回读
    let test_frames = [
        embassy_gd32::can::Frame { id: 0x123, len: 8, data: [0x11,0x22,0x33,0x44,0x55,0x66,0x77,0x88] },
        embassy_gd32::can::Frame { id: 0x456, len: 4, data: [0xAA,0x55,0xAA,0x55,0,0,0,0] },
        embassy_gd32::can::Frame { id: 0x7FF, len: 0, data: [0; 8] }, // 极限：最大 ID + 0 长度
    ];

    let mut pass = 0u32;
    for (i, frame) in test_frames.iter().enumerate() {
        if !can.send(frame) {
            uart.write(b"frame");
            put_hex(&uart, i as u8);
            uart.write(b": SEND FAIL\r\n");
            continue;
        }
        // 回环延迟极短，直接轮询收
        let mut got = false;
        for _ in 0..100_000 {
            if let Some(rx) = can.recv() {
                let ok = rx.id == frame.id
                    && rx.len == frame.len
                    && rx.data[..frame.len as usize] == frame.data[..frame.len as usize];
                if ok {
                    pass += 1;
                    uart.write(b"frame");
                    put_hex(&uart, i as u8);
                    uart.write(b": MATCH id=0x");
                    put_hex(&uart, (frame.id >> 8) as u8);
                    put_hex(&uart, frame.id as u8);
                    uart.write(b"\r\n");
                } else {
                    uart.write(b"frame");
                    put_hex(&uart, i as u8);
                    uart.write(b": MISMATCH\r\n");
                }
                got = true;
                break;
            }
            core::hint::spin_loop();
        }
        if !got {
            uart.write(b"frame");
            put_hex(&uart, i as u8);
            uart.write(b": NO RX\r\n");
        }
        delay_ms(5);
    }

    if pass == 3 {
        uart.write(b"CAN_LOOPBACK: PASS\r\n");
    } else {
        uart.write(b"CAN_LOOPBACK: FAIL\r\n");
    }

    loop {
        core::hint::spin_loop();
    }
}
