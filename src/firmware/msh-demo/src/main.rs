//! msh-demo：需求 3/4/5 三合一板上验收
//!
//! - println!/print!（需求 3）：console::init 后全局可用，多任务共享
//! - 确定性延时（需求 5）：delay_ms/delay_us 经 TIMER1 CNT 差值轮询，
//!   blink 任务（3Hz）+ 延时精度自测命令同时证明
//! - msh shell（需求 4）：命令表驱动（help/echo/delay/blink/reboot），
//!   回显/退格/prompt 全交互形态
#![no_std]
#![no_main]
extern crate alloc;

use alloc::string::String;

use embassy_executor::Spawner;
use embassy_gd32::console::Shell;
use embassy_gd32::{delay_ms, println};
use linked_list_allocator::LockedHeap;
use panic_halt as _;

// shell 的 String 响应走堆分配（alloc feature 进链接图后链接器要求
// 全局分配器符号；4K 足够 shell 场景）
static mut HEAP_MEM: [u8; 4096] = [0; 4096];

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// unsafe 依据：单核单点初始化（main 最前，任务未启动），堆区域独占
fn init_heap() {
    unsafe {
        let mem = core::ptr::addr_of_mut!(HEAP_MEM);
        ALLOCATOR.lock().init((*mem).as_mut_ptr(), (*mem).len());
    }
}

/// 后台任务：LED 闪烁 + 周期心跳（证明 console 与延时在多任务下工作）
#[embassy_executor::task]
async fn blink_task() {
    let p = embassy_gd32::periph_steal();
    let rcc = embassy_gd32::Rcc::new(p.rcu);
    rcc.enable_gpio_port(embassy_gd32::Port::D);
    let mut led = embassy_gd32::Pin::output(p.gpiod, 2);
    loop {
        led.toggle();
        // 异步延时（embassy Timer）：阻塞式 delay_ms 在线程执行器里会
        // 饿死其它任务（板上实测：shell poll_line 永不被调用）——async
        // 任务必须用 .await 让出执行线程
        embassy_time::Timer::after(embassy_time::Duration::from_millis(333)).await;
    }
}

#[embassy_executor::main]
async fn main(sp: Spawner) {
    init_heap();
    // console（println!/msh 载体）+ 确定性延时时基
    // init_time_driver 缺失 = TIMER1 不跑 -> 首个 Timer::after 永挂
    // （stage2g 同款坑：banner 正常但此后全静默）
    embassy_gd32::init_time_driver();
    embassy_gd32::console::init();
    embassy_gd32::init_time_driver();

    println!("msh-demo v1");
    println!("println!/delay/msh three-in-one demo");

    sp.spawn(blink_task().unwrap());

    // msh shell：命令表驱动
    let mut shell = Shell::<8, 128>::new("msh> ");
    shell.add_cmd(embassy_gd32::console::CmdEntry {
        name: "help",
        usage: "list commands",
        handler: |args| help_text(args),
    });
    shell.add_cmd(embassy_gd32::console::CmdEntry {
        name: "echo",
        usage: "echo <text..>",
        handler: |args| {
            let mut s = String::from(if args.len() > 1 { "" } else { "usage: echo <text..>" });
            for t in &args[1..] {
                s.push_str(t);
                s.push(' ');
            }
            s
        },
    });
    shell.add_cmd(embassy_gd32::console::CmdEntry {
        name: "delay",
        usage: "delay <ms> (timing self-test, 1000ms expected)",
        handler: |args| {
            let ms: u32 = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(1000);
            let t0 = embassy_gd32::time_driver::now_us();
            delay_ms(ms);
            let dt = embassy_gd32::time_driver::now_us() - t0;
            alloc::format!("delay {}ms -> actual {}us", ms, dt)
        },
    });
    shell.add_cmd(embassy_gd32::console::CmdEntry {
        name: "reboot",
        usage: "reset to bootloader (re-verify W25Q slot)",
        handler: |_args| {
            embassy_gd32::console::jump_to_bootloader()
        },
    });
    shell.print_prompt();

    loop {
        if let Some(line) = shell.poll_line() {
            shell.exec(&line);
        }
        embassy_time::Timer::after(embassy_time::Duration::from_millis(5)).await;
    }
}

fn help_text(_args: &[&str]) -> String {
    String::from("commands: help/echo/delay/reboot")
}
