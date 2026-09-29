//! USART/UART：串行口抽象（同步轮询 API 先行，async+DMA 后续阶段引入）
//!
//! 引脚映射（权威来源：carrier-box drv_usart.c 实跑配置 + target.md 接口标注，
//! 二者交叉验证）：
//! - PC_RS232_1 = UART6  @ APB1，TX=PE7 / RX=PE8（AF8）  -> /dev/ttyUSB0
//! - RS485_1    = USART1 @ APB1，TX=PA2 / RX=PA3（AF7）+ DE=PD4 -> /dev/ttyUSB1
//! - CPLD_UART  = 经 CPLD uart_mux 切换（与 CPLD 驱动一起做）
//!
//! 寄存器事实（GD32 命名，PAC 布局）：
//! - UART6 复用 uart3::RegisterBlock（简化布局，无 ctl3/rt）
//! - USART1 复用 usart0::RegisterBlock（全量布局）
//! - STAT0：TBE=bit7 / RBNE=bit5 / TC=bit6（手册定案）
//! - BAUD@0x08 = INTDIV[15:4] + FRADIV[3:0]，组合值 = PCLK/baud（oversample16）
//! - BAUD/DATA/AFSEL 字段经 SVD writeConstraint 补丁为 Safe writer

use gd32f470::{uart3, usart0};

/// 生成同类布局的串口 newtype（UART 类与 USART 类寄存器块类型不同，
/// 但 stat0/data/baud/ctl0 四个访问器签名一致，宏去重公共方法）
macro_rules! uart_device {
    ($name:ident, $rb:ty, $doc:expr) => {
        #[doc = $doc]
        pub struct $name<'a> {
            rb: &'a $rb,
        }

        impl<'a> $name<'a> {
            /// 从寄存器块构造（未使能；先备好 GPIO/时钟再调 enable）
            pub fn new(rb: &'a $rb) -> Self {
                Self { rb }
            }

            /// 使能外设：115200-N8-1（oversample16，BAUD=PCLK/baud）
            pub fn enable(&self, pclk_hz: u32, baud: u32) {
                let rb = self.rb;
                // 配置期间关外设（UEN=0 时才可写 BAUD 等）
                rb.ctl0().modify(|_, w| w.uen().clear_bit());

                let reg = (pclk_hz / baud) as u16;
                rb.baud().write(|w| {
                    w.intdiv().set(reg >> 4).fradiv().set((reg & 0xF) as u8)
                });

                // CTL0：8 位字长（WL=0 复位默认）、无校验（PCEN=0）、使能 TX/RX/外设
                rb.ctl0()
                    .modify(|_, w| w.ten().set_bit().ren().set_bit().uen().set_bit());
                // CTL2 复位默认即 1 停止位、无流控
            }

            /// 阻塞写单字节（等 STAT0.TBE；不含移位完成，RS485 场景先 flush 再撤 DE）
            pub fn write_byte(&self, b: u8) {
                while !self.rb.stat0().read().tbe().bit_is_set() {
                    core::hint::spin_loop();
                }
                self.rb.data().write(|w| w.data().set(b as u16));
            }

            /// 等最后一字节移位完成（STAT0.TC）
            pub fn flush(&self) {
                while !self.rb.stat0().read().tc().bit_is_set() {
                    core::hint::spin_loop();
                }
            }

            /// 阻塞写缓冲并等移位完成
            pub fn write(&self, buf: &[u8]) {
                for &b in buf {
                    self.write_byte(b);
                }
                self.flush();
            }

            /// 非阻塞读：RBNE 置位时返回接收字节（读 DATA 自动清 RBNE）。
            /// 帧错误/溢出等错误态按手册顺序清 flags（读 STAT0 后读 DATA）
            pub fn read_byte(&self) -> Option<u8> {
                let st = self.rb.stat0().read();
                if st.orerr().bit_is_set() || st.ferr().bit_is_set()
                    || st.nerr().bit_is_set() || st.perr().bit_is_set()
                {
                    let _ = self.rb.stat0().read();
                    let _ = self.rb.data().read();
                    return None;
                }
                if st.rbne().bit_is_set() {
                    Some(self.rb.data().read().data().bits() as u8)
                } else {
                    None
                }
            }
        }
    };
}

/// UART3/UART6 类布局（UART6 = PC_RS232_1）
uart_device!(
    Uart,
    uart3::RegisterBlock,
    "UART 类串口（UART6 = PC_RS232_1，PE7/PE8；UART6 复用 uart3 布局）"
);

/// USART0/1/2/5 类布局（USART1 = RS485_1，PA2/PA3）
uart_device!(
    Usart,
    usart0::RegisterBlock,
    "USART 类串口（USART1 = RS485_1，PA2/PA3 + DE 方向脚）"
);
