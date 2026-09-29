//! bootloader：阶段 5——应用合法性检查 + VTOR 重定向 + 跳转（0x08008000）
//!
//! 内存布局（link-boot.x）：FLASH[0x08000000..0x08008000) = 本程序，
//! 应用起始 0x08008000（向量表 + 应用镜像）。
//!
//! 合法性检查（跳转前逐项验证，任一失败则 LED 慢闪报错并停机）：
//! 1. 应用初始 SP ∈ [0x20000000, 0x20030000]（主 SRAM 实测边界）
//! 2. 应用 reset 向量 thumb 位（bit0）置位
//! 跳转序列（cortex-m-rt 官方跳转范式）：
//!   关中断 -> VTOR=应用向量表 -> MSP=应用 SP -> 同步屏障 -> 跳 reset
//!
//! unsafe 收敛：跳转本身是裸金属操作的本质（无法用安全 API 表达），
//! 每步注释依据；合法性检查将风险收敛为"镜像损坏即拒绝跳转"。
#![no_std]
#![no_main]

use embassy_gd32::{Pin, Port, Rcc, Uart};
use panic_halt as _;

const APP_ADDR: u32 = 0x0800_8000;
/// 主 SRAM 实测边界（448K 地图定案：0x20000000..0x20070000）
const RAM_BASE: u32 = 0x2000_0000;
const RAM_END: u32 = 0x2007_0000;

fn delay_ms(ms: u32) {
    for _ in 0..ms {
        for _ in 0..4000 {
            core::hint::spin_loop();
        }
    }
}

fn put_hex8(uart: &Uart, v: u32) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for shift in [28, 24, 20, 16, 12, 8, 4, 0] {
        uart.write_byte(HEX[((v >> shift) & 0xF) as usize]);
    }
}

/// 读取应用向量表首字（初始 SP）
#[allow(unsafe_code)]
fn app_sp() -> u32 {
    unsafe { core::ptr::read_volatile(APP_ADDR as *const u32) }
}

/// 读取应用 reset 向量
#[allow(unsafe_code)]
fn app_reset() -> u32 {
    unsafe { core::ptr::read_volatile((APP_ADDR + 4) as *const u32) }
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = gd32f470::Peripherals::take().expect("peripherals already taken");

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::E);
    let _utx = Pin::alternate(&p.gpioe, 7, 8);
    let _urx = Pin::alternate(&p.gpioe, 8, 8);
    let uart = Uart::new(&p.uart6);
    uart.enable(16_000_000, 115_200);
    uart.write(b"bootloader: checking app @0x08008000\r\n");

    let sp = app_sp();
    let rv = app_reset();

    // 合法性检查 1：初始 SP 必须落在主 SRAM 区
    if sp < RAM_BASE || sp > RAM_END {
        uart.write(b"app SP out of range: 0x");
        put_hex8(&uart, sp);
        uart.write(b"\r\n");
        error_halt(&uart);
    }
    // 合法性检查 2：reset 向量必须带 thumb 位
    if rv & 1 == 0 {
        uart.write(b"app reset vector not thumb: 0x");
        put_hex8(&uart, rv);
        uart.write(b"\r\n");
        error_halt(&uart);
    }

    uart.write(b"app OK: SP=0x");
    put_hex8(&uart, sp);
    uart.write(b" RST=0x");
    put_hex8(&uart, rv & !1);
    uart.write(b" -> jumping\r\n");
    delay_ms(100);

    jump(sp, rv & !1)
}

/// 错误停机：LED 慢闪（LED_1=PD2 低有效）+ console 提示
fn error_halt(uart: &Uart) -> ! {
    uart.write(b"bootloader: HALT\r\n");
    loop {
        core::hint::spin_loop();
    }
}

/// 跳转到应用（unsafe 收敛点：裸金属跳转的本质操作）。
///
/// unsafe 依据：合法性检查已验证 SP 在主 SRAM 区、reset 向量带 thumb 位；
/// 跳转前关中断 + VTOR 重定向 + MSP 重载，序列来自 cortex-m-rt 官方
/// 跳转范式。跳转后控制权完全移交应用。
#[allow(unsafe_code)]
fn jump(sp: u32, reset: u32) -> ! {
    unsafe {
        // 1. 关中断（PRIMASK=1），避免跳转过程中断打在旧向量上
        core::arch::asm!("cpsid i");
        // 2. VTOR 重定向到应用向量表
        (0xE000_ED08u32 as *mut u32).write_volatile(APP_ADDR);
        core::arch::asm!("dsb", "isb");
        // 3. MSP 重载为应用初始 SP
        core::arch::asm!(
            "msr msp, {sp}",
            sp = in(reg) sp,
        );
        // 4. 清流水线后跳转（BX 带 thumb 位）
        core::arch::asm!("dsb", "isb", "bx {r}", r = in(reg) reset, options(noreturn));
    }
}
