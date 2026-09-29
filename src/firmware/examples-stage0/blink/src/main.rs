//! 阶段0 bring-up：GD32F470VGT6 裸机 blink，手写寄存器（无 PAC 依赖）
//! LED_1 = PD2（原理图 pcb-00357，低电平点亮，用户已核对实物）
//! 验证：时钟默认 HSI(16MHz)/复位态、GPIO 挂 AHB1、probe-rs 烧录链路
#![no_std]
#![no_main]

use core::ptr::{read_volatile, write_volatile};

const PERIPH_BASE: u32 = 0x4000_0000;
const AHB1_BASE: u32 = PERIPH_BASE + 0x0002_0000;

const RCU_BASE: u32 = PERIPH_BASE + 0x0002_8000 + 0x0000_0000; // GD32F4 RCU @ 0x40023800
const RCU_AHB1EN: *mut u32 = (RCU_BASE + 0x30) as *mut u32;

const GPIOD_BASE: u32 = AHB1_BASE + 0x0C00; // GPIOD @ 0x40020C00
const GPIOD_MODER: *mut u32 = (GPIOD_BASE + 0x00) as *mut u32;
const GPIOD_OTYPER: *mut u32 = (GPIOD_BASE + 0x04) as *mut u32;
const GPIOD_OSPEEDR: *mut u32 = (GPIOD_BASE + 0x08) as *mut u32;
const GPIOD_PUPDR: *mut u32 = (GPIOD_BASE + 0x0C) as *mut u32;
const GPIOD_ODR: *mut u32 = (GPIOD_BASE + 0x14) as *mut u32;

const LED1_PIN: u8 = 2;

#[cortex_m_rt::entry]
fn main() -> ! {
    unsafe {
        // RCU_AHB1EN bit3 = GPIODEN
        write_volatile(RCU_AHB1EN, read_volatile(RCU_AHB1EN) | (1 << 3));
        // 读回同步时钟使能（GD32 手册要求写后读）
        let _ = read_volatile(RCU_AHB1EN);

        // MODER1 = 01 输出
        let m = read_volatile(GPIOD_MODER) & !(0b11 << (LED1_PIN * 2));
        write_volatile(GPIOD_MODER, m | (0b01 << (LED1_PIN * 2)));
        // 推挽
        write_volatile(GPIOD_OTYPER, read_volatile(GPIOD_OTYPER) & !(1 << LED1_PIN));
        // OSPEED 低速、无上下拉
        write_volatile(GPIOD_OSPEEDR, read_volatile(GPIOD_OSPEEDR) & !(0b11 << (LED1_PIN * 2)));
        write_volatile(GPIOD_PUPDR, read_volatile(GPIOD_PUPDR) & !(0b11 << (LED1_PIN * 2)));

        loop {
            write_volatile(GPIOD_ODR, read_volatile(GPIOD_ODR) ^ (1 << LED1_PIN));
            delay(2_000_000);
        }
    }
}

/// 粗略忙等延时（默认 HSI 16MHz，未配 SYSTICK 阶段0够用）
fn delay(n: u32) {
    for i in 0..n {
        core::hint::black_box(i);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
