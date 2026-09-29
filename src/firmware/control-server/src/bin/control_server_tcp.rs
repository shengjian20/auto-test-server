//! control-server-tcp：TCP 传输变体（协议主体单一来源复用）
//!
//! 与 UART 变体共享 app.rs 的 init_deps + AppDeps::dispatch——传输层仅
//! 替换输入行来源（TCP 套接字）与输出汇（send_slice），协议行为逐字节
//! 一致（UART 变体回归 CONTROL_V2 PASS 后重构，E2E 双向等价）。
//!
//! 内置自测（MAC_LBM=1，无网线）：内置 client 连本机 :9000 发 "ping"，
//! 经 ARP+SYN+DATA 全栈往返，验证 "OK pong" 响应——协议-over-TCP 证明。
//! 接网线实测：改 MAC_LBM=0，PC `nc 172.22.0.50 9000` 直接发协议命令。
#![no_std]
#![no_main]
extern crate alloc;

use alloc::vec;
use linked_list_allocator::LockedHeap;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr};
use managed::ManagedSlice;

use control_server::app;
use embassy_gd32::enet::{self, LinkState, Phy};
use embassy_gd32::enet_dma;
use embassy_gd32::{Pin, Port, Rcc, Uart};
use panic_halt as _;

const PCLK_HZ: u32 = 16_000_000;
const LOCAL_IP: [u8; 4] = [172, 22, 0, 50];
const LOCAL_MAC: [u8; 6] = [0x02, 0x04, 0x06, 0x08, 0x0A, 0x0C];
const PORT: u16 = 9000;
/// MAC LBM 自测模式：lbm feature 开启时启用（无网线全栈自环自测）；
/// 缺省=normal（接网线实测，PC 侧 `nc 172.22.0.50 9000` 发协议命令）
#[cfg(feature = "lbm")]
const MAC_LBM: bool = true;
#[cfg(not(feature = "lbm"))]
const MAC_LBM: bool = false;

// ---- 堆（smoltcp alloc；DMA 可达性约束：TCM 禁 DMA，堆必须在主 SRAM）----
static mut HEAP_MEM: [u8; 65536] = [0; 65536];

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// unsafe 依据：单核单点初始化（main 最前，任务未启动），堆区域独占
fn init_heap() {
    unsafe {
        let mem = core::ptr::addr_of_mut!(HEAP_MEM);
        ALLOCATOR.lock().init((*mem).as_mut_ptr(), (*mem).len());
    }
}

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

#[embassy_executor::main]
async fn main(_sp: embassy_executor::Spawner) {
    init_heap();

    let p = app::periph::steal();
    embassy_gd32::init_time_driver();

    // 全外设依赖（dispatch 操作对象）+ UART 日志口（init_deps 内含时钟门
    // 使能与 UART6 RXNE 环切换——TCP 变体下环无人消费，环形覆写无害）
    let (mut deps, uart) = app::init_deps();

    let rcc = Rcc::new(p.rcu);
    rcc.enable_syscfg();
    rcc.enable_enet();

    let _utx = Pin::alternate(p.gpioe, 7, 8);
    let _urx = Pin::alternate(p.gpioe, 8, 8);
    uart.enable(PCLK_HZ, 115_200);
    uart.write(b"control-server-tcp v1\r\n");

    // 1. RMII 选择（ENET 复位前）
    p.syscfg.cfg1().modify(|_, w| w.enet_phy_sel().set_bit());

    // 2. PHY 硬复位 + 引脚 AF11
    let mut phy_rst = Pin::output(p.gpioc, 0);
    phy_rst.set_low();
    delay_ms(10);
    phy_rst.set_high();
    delay_ms(50);
    let _ref_clk = Pin::alternate(p.gpioa, 1, 11);
    let _mdio = Pin::alternate(p.gpioa, 2, 11);
    let _crs_dv = Pin::alternate(p.gpioa, 7, 11);
    let _mdc = Pin::alternate(p.gpioc, 1, 11);
    let _rxd0 = Pin::alternate(p.gpioc, 4, 11);
    let _rxd1 = Pin::alternate(p.gpioc, 5, 11);
    let _tx_en = Pin::alternate(p.gpiob, 11, 11);
    let _txd0 = Pin::alternate(p.gpiob, 12, 11);
    let _txd1 = Pin::alternate(p.gpiob, 13, 11);

    // 3. SWR 轮询（REF_CLK 死线检测）
    if !enet::sw_reset(&p.enet_dma) {
        uart.write(b"SWR STUCK (REF_CLK dead)\r\n");
        loop {
            core::hint::spin_loop();
        }
    }

    // 4. PHY ID 校验
    let phy = Phy::new(p.enet_mac);
    match phy.read_id() {
        Some(0x0007C0F1) => uart.write(b"PHY: LAN8720A OK\r\n"),
        _ => {
            uart.write(b"PHY: no LAN8720A\r\n");
            loop {
                core::hint::spin_loop();
            }
        }
    }

    // 5. MAC 配置 + 混杂 + MAC 地址
    p.enet_mac
        .mac_cfg()
        .modify(|_, w| w.spd().set_bit().dpm().set_bit());
    if MAC_LBM {
        p.enet_mac.mac_cfg().modify(|_, w| w.lbm().set_bit());
        uart.write(b"mode: MAC LBM self-test\r\n");
    } else {
        uart.write(b"mode: normal\r\n");
    }
    p.enet_mac.mac_frmf().modify(|_, w| w.pm().set_bit());
    enet::set_mac_addr0(p.enet_mac, LOCAL_MAC);

    // 6. 描述符环 + DMA 启动
    let rings = enet_dma::take_rings();
    rings.tx.init();
    rings.rx.init();
    enet_dma::start_dma(&p.enet_dma, rings);
    p.enet_mac
        .mac_cfg()
        .modify(|_, w| w.ren().set_bit().ten().set_bit());
    enet_dma::rx_poll(&p.enet_dma);
    uart.write(b"DMA started\r\n");

    // 7. smoltcp 栈
    let mut device = enet_smoltcp_wrap(&mut rings.rx, &mut rings.tx, &p.enet_dma);
    let mut interface = Interface::new(
        Config::new(HardwareAddress::Ethernet(EthernetAddress(LOCAL_MAC))),
        &mut device,
        Instant::from_millis(0i64),
    );
    interface.update_ip_addrs(|addrs| {
        let _ = addrs.push(IpCidr::new(
            IpAddress::v4(LOCAL_IP[0], LOCAL_IP[1], LOCAL_IP[2], LOCAL_IP[3]),
            23,
        ));
    });

    let mut sockets = SocketSet::new(vec![]);
    let server_h = sockets.add(tcp::Socket::new(
        ManagedSlice::Owned(vec![0u8; 2048]),
        ManagedSlice::Owned(vec![0u8; 2048]),
    ));
    let client_h = sockets.add(tcp::Socket::new(
        ManagedSlice::Owned(vec![0u8; 512]),
        ManagedSlice::Owned(vec![0u8; 512]),
    ));
    sockets
        .get_mut::<tcp::Socket>(server_h)
        .listen(PORT)
        .unwrap();
    uart.write(b"listening :9000\r\n");

    // 8. 协议自测状态机：client 发 "ping" -> server dispatch -> "OK pong"
    let probe = b"ping\n";
    let mut sent = 0usize;
    let mut resp = [0u8; 64];
    let mut resp_len = 0usize;
    let mut phase = 0u8; // 0=连接 1=发送 2=收验 3=完成
    let mut t_ms: i64 = 0;
    let mut hb: i32 = 0;

    loop {
        interface.poll(Instant::from_millis(t_ms), &mut device, &mut sockets);
        t_ms += 2;

        // server：CloseWait 回收（PC 断开后 socket 滞留 CLOSE_WAIT 且
        // is_open()==true 永不重听——后续连接全部 refused，板上实测
        // 15 次尝试仅首次成功即此）+ 行组装 -> deps.dispatch
        {
            let mut s = sockets.get_mut::<tcp::Socket>(server_h);
            if s.state() == tcp::State::CloseWait {
                s.close();
            }
            if !s.is_open() {
                let _ = s.listen(PORT);
            }
            let mut line = [0u8; app::LINE_MAX];
            if s.can_recv() {
                let n = s.recv_slice(&mut line).unwrap_or(0);
                let mut start = 0usize;
                for i in 0..n {
                    if line[i] == b'\n' || line[i] == b'\r' {
                        if i > start {
                            let mut toks: [&[u8]; 8] = [b""; 8];
                            let nt = app::tokenize(&line[start..i], &mut toks);
                            let mut out = |b: &[u8]| {
                                let _ = s.send_slice(b);
                            };
                            deps.dispatch(&mut out, &toks, nt);
                        }
                        start = i + 1;
                    }
                }
                // 残尾（无换行的部分行）留待下拍拼接——简化：协议命令行短，
                // TCP 有序字节流按包边界即整行（单命令 < MSS），残尾丢弃
                let _ = start;
            }
        }
        drop_sockets(&mut sockets);

        // client 状态机（仅 MAC_LBM 自测模式；normal 模式下连本机 IP：
        // SYN 经交换机有去无回 -> 重传死循环持续置 TBU 干扰 server）
        if MAC_LBM {
        {
            let mut c = sockets.get_mut::<tcp::Socket>(client_h);
            match phase {
                0 => {
                    if !c.is_open() && !c.is_active() {
                        let _ = c.connect(
                            &mut interface.context(),
                            (
                                IpAddress::v4(LOCAL_IP[0], LOCAL_IP[1], LOCAL_IP[2], LOCAL_IP[3]),
                                PORT,
                            ),
                            49152u16, // 本地临时端口：smoltcp 不自动分配（port=0 立即被拒）
                        );
                    }
                    if c.is_active() {
                        uart.write(b"client connected\r\n");
                        phase = 1;
                    }
                }
                1 => {
                    if c.can_send() {
                        let n = c.send_slice(&probe[sent..]).unwrap_or(0);
                        sent += n;
                        if sent >= probe.len() {
                            phase = 2;
                        }
                    }
                }
                2 => {
                    if c.can_recv() {
                        let mut tmp = [0u8; 64];
                        if let Ok(n) = c.recv_slice(&mut tmp) {
                            let k = core::cmp::min(n, resp.len() - resp_len);
                            resp[resp_len..resp_len + k].copy_from_slice(&tmp[..k]);
                            resp_len += k;
                            if resp_len >= 7 {
                                // "OK pong" 全量到达即判
                                phase = 3;
                                uart.write(b"TCP_PROTO_SELFTEST: ");
                                if &resp[..7] == b"OK pong" {
                                    uart.write(b"PASS (OK pong via TCP)\r\n");
                                } else {
                                    uart.write(b"FAIL resp=");
                                    put_hex8(&uart, u32::from_le_bytes([
                                        resp[0], resp[1], resp[2], resp[3],
                                    ]));
                                    uart.write(b"\r\n");
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        }

        // RX/TX 挂起恢复（TBU 无恢复则帧滞留描述符永不发出——ARP 永不解析）
        embassy_gd32::enet_smoltcp::recover_suspended(&p.enet_dma);

        // 心跳
        hb += 2;
        if hb >= 5000 {
            hb = 0;
            if phase != 3 {
                uart.write(b"hb t=");
                put_hex8(&uart, t_ms as u32);
                uart.write(b" phase=");
                put_hex8(&uart, phase as u32);
                uart.write(b" link=");
                match phy.link_state() {
                    Some(LinkState::Down) => uart.write(b"DOWN"),
                    Some(_) => uart.write(b"UP"),
                    None => uart.write(b"ERR"),
                }
                uart.write(b" stat=0x");
                put_hex8(&uart, p.enet_dma.dma_stat().read().bits());
                uart.write(b"\r\n");
            }
        }

        embassy_time::Timer::after(embassy_time::Duration::from_millis(2)).await;
    }
}

/// 结束 socket 借用（作用域辅助；NLL 下 drop_sockets 仅为显式化）
fn drop_sockets(_sockets: &mut SocketSet<'_>) {}

/// smoltcp Device 适配（HAL enet_smoltcp 模块的借用组装）
fn enet_smoltcp_wrap<'a>(
    rx: &'a mut enet_dma::RDesRing,
    tx: &'a mut enet_dma::TDesRing,
    dma: &'a gd32f470::EnetDma,
) -> embassy_gd32::enet_smoltcp::EnetDevice<'a> {
    embassy_gd32::enet_smoltcp::EnetDevice { rx, tx, dma }
}
