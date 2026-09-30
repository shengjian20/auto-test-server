//! tcp-echo：阶段 6——smoltcp TCP 服务器对接 enet_dma 描述符环
//!
//! E2E 自测形态（MAC_LBM=1，无网线）：单任务双 socket 状态机——
//! server 监听 :9000，client 连本机 172.22.0.50:9000，client 发 256B
//! 模式串 -> server 回显 -> client 比对 -> UART 打印 PASS/FAIL。
//! MAC LBM 在 MII 层内部回环（TX->RX），ARP/SYN/DATA 全栈自发自收。
//! 接网线实测：改 MAC_LBM=0 重新构建，PC `nc 172.22.0.50 9000` 回显。
//!
//! 官方配置流程（UM 27.3.7）：SYSCFG RMII -> 四时钟门 -> PHY 复位 ->
//! 引脚 AF11 -> SWR 轮询 -> PHY ID 校验 -> MAC CFG -> 描述符环 -> DMA 启动
//!
//! alloc 路线：smoltcp 0.11 的 SocketSet/Socket 缓冲经 Vec（Into<ManagedSlice>
//! 仅 alloc 版本可用），堆用 linked_list_allocator 放主 SRAM 192K 区
//! （.bss；TCM 禁 DMA——板上定案，DMA 收发缓冲禁入堆）
#![no_std]
#![no_main]
extern crate alloc;

use alloc::vec;
use linked_list_allocator::LockedHeap;
use managed::ManagedSlice;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, Ipv4Address};

use embassy_gd32::enet::{self, LinkState, Phy};
use embassy_gd32::enet_dma;
use embassy_gd32::{Pin, Port, Rcc, Uart};
use panic_halt as _;
use core::sync::atomic::{AtomicU32, Ordering};

static RX_HITS: AtomicU32 = AtomicU32::new(0);
static TX_COMMITS: AtomicU32 = AtomicU32::new(0);
static TX_COMMIT_FAILS: AtomicU32 = AtomicU32::new(0);

const PCLK_HZ: u32 = 16_000_000;
/// 板卡静态 IP（与宿主机 enp0s31f6 172.22.0.194/23 同网段）
const LOCAL_IP: [u8; 4] = [172, 22, 0, 50];
const LOCAL_MAC: [u8; 6] = [0x02, 0x04, 0x06, 0x08, 0x0A, 0x0C];
const PORT: u16 = 9000;
/// MAC LBM 自测模式（无网线）；接网线实测时改 0
/// MAC LBM 自测模式：lbm feature 开启时启用（无网线全栈自环自测）；
/// 缺省=normal（接网线实测，PC 侧 `nc 172.22.0.50 9000` 发数据回显）
#[cfg(feature = "lbm")]
const MAC_LBM: bool = true;
#[cfg(not(feature = "lbm"))]
const MAC_LBM: bool = false;
/// 自测模式串长度
const PATTERN_LEN: usize = 256;

// ---- 堆（smoltcp alloc 路线；DMA 可达性约束：TCM 禁 DMA，堆必须在主 SRAM）----
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

// ---- smoltcp Device 适配层 ----

pub struct EnetDevice<'a> {
    rx: &'a mut enet_dma::RDesRing,
    tx: &'a mut enet_dma::TDesRing,
}

pub struct EnetRxToken<'a> {
    len: usize,
    rx: &'a mut enet_dma::RDesRing,
    idx: usize,
}

impl RxToken for EnetRxToken<'_> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        // FL 含 4 字节 FCS，剥离后经栈上 scratch 交付（描述符立即归还，
        // 避免环内 buf 被 f 闭包借用导致的复用悬垂）
        let n = self.len.min(enet_dma::BUF_SIZE);
        let mut scratch = [0u8; enet_dma::BUF_SIZE];
        scratch[..n].copy_from_slice(&self.rx.buf[self.idx][..n]);
        self.rx.release(self.idx);
        f(&mut scratch[..n])
    }
}

pub struct EnetTxToken<'a> {
    tx: &'a mut enet_dma::TDesRing,
}

impl TxToken for EnetTxToken<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let i = self.tx.index;
        let r = f(&mut self.tx.buf[i][..len]);
        if self.tx.tx_commit(len) {
            TX_COMMITS.fetch_add(1, Ordering::Relaxed);
        } else {
            TX_COMMIT_FAILS.fetch_add(1, Ordering::Relaxed);
        }
        r
    }
}

impl Device for EnetDevice<'_> {
    type RxToken<'a>
        = EnetRxToken<'a>
    where
        Self: 'a;
    type TxToken<'a>
        = EnetTxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let idx = self.rx.first_valid()?;
        RX_HITS.fetch_add(1, Ordering::Relaxed);
        let fl = self.rx.frame_len(idx);
        let len = fl.saturating_sub(4).min(enet_dma::BUF_SIZE); // 剥 FCS
        let rx = &mut *self.rx;
        let tx = &mut *self.tx;
        Some((EnetRxToken { len, rx, idx }, EnetTxToken { tx }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if self.tx.available() {
            Some(EnetTxToken { tx: &mut *self.tx })
        } else {
            None
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(enet_dma::RING_LEN as usize);
        caps
    }
}

#[embassy_executor::main]
async fn main(_sp: embassy_executor::Spawner) {
    init_heap();

    let p = gd32f470::Peripherals::take().expect("peripherals already taken");
    embassy_gd32::init_time_driver();

    let rcc = Rcc::new(&p.rcu);
    rcc.enable_gpio_port(Port::A);
    rcc.enable_gpio_port(Port::B);
    rcc.enable_gpio_port(Port::C);
    rcc.enable_gpio_port(Port::E);
    rcc.enable_uart6();
    rcc.enable_syscfg();
    rcc.enable_enet();

    let _utx = Pin::alternate(&p.gpioe, 7, 8);
    let _urx = Pin::alternate(&p.gpioe, 8, 8);
    let uart = Uart::new(&p.uart6);
    uart.enable(PCLK_HZ, 115_200);
    uart.write(b"tcp-echo v1 (alloc)\r\n");

    // 1. RMII 选择（ENET 复位前）
    p.syscfg.cfg1().modify(|_, w| w.enet_phy_sel().set_bit());

    // 2. PHY 硬复位 + 引脚 AF11
    let mut phy_rst = Pin::output(&p.gpioc, 0);
    phy_rst.set_low();
    delay_ms(10);
    phy_rst.set_high();
    delay_ms(50);
    let _ref_clk = Pin::alternate(&p.gpioa, 1, 11);
    let _mdio = Pin::alternate(&p.gpioa, 2, 11);
    let _crs_dv = Pin::alternate(&p.gpioa, 7, 11);
    let _mdc = Pin::alternate(&p.gpioc, 1, 11);
    let _rxd0 = Pin::alternate(&p.gpioc, 4, 11);
    let _rxd1 = Pin::alternate(&p.gpioc, 5, 11);
    let _tx_en = Pin::alternate(&p.gpiob, 11, 11);
    let _txd0 = Pin::alternate(&p.gpiob, 12, 11);
    let _txd1 = Pin::alternate(&p.gpiob, 13, 11);

    // 3. SWR 轮询（REF_CLK 死线检测）
    if !enet::sw_reset(&p.enet_dma) {
        uart.write(b"SWR STUCK (REF_CLK dead)\r\n");
        loop {
            core::hint::spin_loop();
        }
    }

    // 4. PHY ID 校验
    let phy = Phy::new(&p.enet_mac);
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
    enet::set_mac_addr0(&p.enet_mac, LOCAL_MAC);

    // 6. 描述符环 + DMA 启动
    let rings = enet_dma::take_rings();
    rings.tx.init();
    rings.rx.init();
    enet_dma::start_dma(&p.enet_dma, rings);
    p.enet_mac
        .mac_cfg()
        .modify(|_, w| w.ren().set_bit().ten().set_bit());
    enet_dma::rx_poll(&p.enet_dma); // SRE 置位后的首次 RX 描述符取发
    uart.write(b"DMA started\r\n");

    // 7. smoltcp 栈（alloc 路线：Vec 套接字缓冲）
    let mut device = EnetDevice {
        rx: &mut rings.rx,
        tx: &mut rings.tx,
    };
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
    let _ = interface
        .routes_mut()
        .add_default_ipv4_route(Ipv4Address::new(172, 22, 0, 1));

    let mut sockets = SocketSet::new(alloc::vec::Vec::new());
    let server_h = sockets.add(tcp::Socket::new(
        ManagedSlice::Owned(vec![0u8; 2048]),
        ManagedSlice::Owned(vec![0u8; 2048]),
    ));
    let client_h = sockets.add(tcp::Socket::new(
        ManagedSlice::Owned(vec![0u8; 1024]),
        ManagedSlice::Owned(vec![0u8; 1024]),
    ));
    sockets
        .get_mut::<tcp::Socket>(server_h)
        .listen(PORT)
        .unwrap();
    uart.write(b"listening :9000\r\n");

    // 8. 自测状态机：client 连本机 server，256B 模式串回环比对
    let mut pattern = [0u8; PATTERN_LEN];
    for (i, b) in pattern.iter_mut().enumerate() {
        *b = i as u8;
    }
    let mut echo_buf = [0u8; PATTERN_LEN];
    let mut echo_len = 0usize;
    let mut sent = 0usize;
    let mut phase = 0u8; // 0=连接中 1=发送中 2=收比对中 3=完成
    let mut t_ms: i64 = 0;
    let mut hb: i32 = 0;

    loop {
        interface.poll(Instant::from_millis(t_ms), &mut device, &mut sockets);
        t_ms += 2;

        // server：监听 + 回显（recv_slice 避免 recv 闭包的双可变借用）
        {
            let mut s = sockets.get_mut::<tcp::Socket>(server_h);
            if !s.is_open() {
                let _ = s.listen(PORT);
            }
            if s.can_recv() {
                let mut tmp = [0u8; 512];
                if let Ok(n) = s.recv_slice(&mut tmp) {
                    let _ = s.send_slice(&tmp[..n]);
                }
            }
        }

        // client 状态机
        {
            let mut c = sockets.get_mut::<tcp::Socket>(client_h);
            match phase {
                0 => {
                    if !c.is_open() && !c.is_active() {
                        // 本地端点：smoltcp 0.11 的 Some(0.0.0.0) 走显式
                        // 未指定分支被拒（板上实测 connect ERR 连刷），须用
                        // IpListenEndpoint::from(port) 形态（None 地址 ->
                        // cx.get_source_address 自动选源）
                        let r = c.connect(
                            &mut interface.context(),
                            (
                                IpAddress::v4(LOCAL_IP[0], LOCAL_IP[1], LOCAL_IP[2], LOCAL_IP[3]),
                                PORT,
                            ),
                            49152u16, // 本地临时端口（smoltcp 不自动分配端口）
                        );
                        if r.is_err() {
                            uart.write(b"connect ERR\r\n");
                        } else {
                            uart.write(b"connect OK (syn queued)\r\n");
                        }
                    }
                    if c.is_active() {
                        uart.write(b"client connected\r\n");
                        phase = 1;
                    }
                }
                1 => {
                    if c.can_send() {
                        let n = c.send_slice(&pattern[sent..]).unwrap_or(0);
                        sent += n;
                        if sent >= PATTERN_LEN {
                            phase = 2;
                        }
                    }
                }
                2 => {
                    if c.can_recv() {
                        let mut tmp = [0u8; 256];
                        if let Ok(n) = c.recv_slice(&mut tmp) {
                            let k = core::cmp::min(n, PATTERN_LEN - echo_len);
                            echo_buf[echo_len..echo_len + k].copy_from_slice(&tmp[..k]);
                            echo_len += k;
                            if echo_len >= PATTERN_LEN {
                                phase = 3;
                                uart.write(b"TCP_SELFTEST: ");
                                if echo_buf[..] == pattern[..] {
                                    uart.write(b"PASS (256/256)\r\n");
                                } else {
                                    let mut bad = 0usize;
                                    for i in 0..PATTERN_LEN {
                                        if echo_buf[i] != pattern[i] {
                                            bad += 1;
                                        }
                                    }
                                    put_hex8(&uart, bad as u32);
                                    uart.write(b" bytes mismatch\r\n");
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // RX 挂起恢复：RBU（描述符耗尽）时清标志 + 重 poll（无条件写 RPEN
        // 会在 Running 态引发描述符重取扰动，条件化更稳）
        if p.enet_dma.dma_stat().read().rbu().bit_is_set() {
            p.enet_dma.dma_stat().write(|w| w.rbu().clear_bit());
            enet_dma::rx_poll(&p.enet_dma);
        }

        // TX 挂起恢复：TBU（TX DMA 取不到描述符即停等）时清标志 + poll-demand。
        // 缺失则 smoltcp 提交的帧滞留描述符永不发出（板上实测：ARP 永不解析，
        // client connect 永挂 phase=0，stat TP=6 suspended + TBU）
        if p.enet_dma.dma_stat().read().tbu().bit_is_set() {
            p.enet_dma.dma_stat().write(|w| w.tbu().clear_bit());
            enet_dma::tx_poll(&p.enet_dma);
        }

        // 心跳（每 5s：阶段 + 链路态 + DMA_STAT）
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
                uart.write(b" rx=");
                put_hex8(&uart, RX_HITS.load(Ordering::Relaxed));
                uart.write(b" tx=");
                put_hex8(&uart, TX_COMMITS.load(Ordering::Relaxed));
                uart.write(b" txf=");
                put_hex8(&uart, TX_COMMIT_FAILS.load(Ordering::Relaxed));
                uart.write(b"\r\n");
            }
        }

        embassy_time::Timer::after(embassy_time::Duration::from_millis(2)).await;
    }
}
