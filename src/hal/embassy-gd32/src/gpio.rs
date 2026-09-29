//! GPIO：全端口引脚抽象（同步 API，embedded-hal 1.0）
//!
//! - 引脚号运行期存储；寄存器字段访问经 `pin_write!`/`pin_modify!`/`pin_read_bit!`
//!   宏展开 16 路 match 分发表（PAC 字段 writer 按引脚号命名，如 bop2/cr2/tg2，
//!   无 const 泛型索引手段，宏内 `paste!` 拼接是表驱动去重的最小形式）
//! - 仅使用 PAC 字段级安全 API：svd2rust 0.37 的整寄存器 `W::bits()` 为 unsafe，
//!   本模块不使用；`unreachable!` 分支由构造路径保证（u8 引脚号仅 0-15 可达）

use core::convert::Infallible;
use embedded_hal::digital::{ErrorType, InputPin, OutputPin};
use gd32f470::gpioc;

/// GPIO 端口（GD32F470VGT6 实有 A-I）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
}

/// 引脚工作模式（CTLx 2bit，GD 命名；枚举变体由 SVD 补丁生成）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinMode {
    Input,
    Output,
    Alternate,
    Analog,
}

/// 寄存器写（write 闭包）：按引脚号分发到 `$fld{n}` 字段
macro_rules! pin_write {
    ($rb:expr, $n:expr, $reg:ident, |$w:ident| $fld:ident.$method:ident()) => {
        paste::paste! {
            match $n {
                0 => $rb.$reg().write(|$w| $w.[<$fld 0>]().$method()),
                1 => $rb.$reg().write(|$w| $w.[<$fld 1>]().$method()),
                2 => $rb.$reg().write(|$w| $w.[<$fld 2>]().$method()),
                3 => $rb.$reg().write(|$w| $w.[<$fld 3>]().$method()),
                4 => $rb.$reg().write(|$w| $w.[<$fld 4>]().$method()),
                5 => $rb.$reg().write(|$w| $w.[<$fld 5>]().$method()),
                6 => $rb.$reg().write(|$w| $w.[<$fld 6>]().$method()),
                7 => $rb.$reg().write(|$w| $w.[<$fld 7>]().$method()),
                8 => $rb.$reg().write(|$w| $w.[<$fld 8>]().$method()),
                9 => $rb.$reg().write(|$w| $w.[<$fld 9>]().$method()),
                10 => $rb.$reg().write(|$w| $w.[<$fld 10>]().$method()),
                11 => $rb.$reg().write(|$w| $w.[<$fld 11>]().$method()),
                12 => $rb.$reg().write(|$w| $w.[<$fld 12>]().$method()),
                13 => $rb.$reg().write(|$w| $w.[<$fld 13>]().$method()),
                14 => $rb.$reg().write(|$w| $w.[<$fld 14>]().$method()),
                15 => $rb.$reg().write(|$w| $w.[<$fld 15>]().$method()),
                _ => unreachable!("pin number is 0-15"),
            }
        }
    };
}

/// 寄存器读改写（modify 闭包）：按引脚号分发
macro_rules! pin_modify {
    ($rb:expr, $n:expr, $reg:ident, |$w:ident| $fld:ident.$method:ident()) => {
        paste::paste! {
            match $n {
                0 => $rb.$reg().modify(|_, $w| $w.[<$fld 0>]().$method()),
                1 => $rb.$reg().modify(|_, $w| $w.[<$fld 1>]().$method()),
                2 => $rb.$reg().modify(|_, $w| $w.[<$fld 2>]().$method()),
                3 => $rb.$reg().modify(|_, $w| $w.[<$fld 3>]().$method()),
                4 => $rb.$reg().modify(|_, $w| $w.[<$fld 4>]().$method()),
                5 => $rb.$reg().modify(|_, $w| $w.[<$fld 5>]().$method()),
                6 => $rb.$reg().modify(|_, $w| $w.[<$fld 6>]().$method()),
                7 => $rb.$reg().modify(|_, $w| $w.[<$fld 7>]().$method()),
                8 => $rb.$reg().modify(|_, $w| $w.[<$fld 8>]().$method()),
                9 => $rb.$reg().modify(|_, $w| $w.[<$fld 9>]().$method()),
                10 => $rb.$reg().modify(|_, $w| $w.[<$fld 10>]().$method()),
                11 => $rb.$reg().modify(|_, $w| $w.[<$fld 11>]().$method()),
                12 => $rb.$reg().modify(|_, $w| $w.[<$fld 12>]().$method()),
                13 => $rb.$reg().modify(|_, $w| $w.[<$fld 13>]().$method()),
                14 => $rb.$reg().modify(|_, $w| $w.[<$fld 14>]().$method()),
                15 => $rb.$reg().modify(|_, $w| $w.[<$fld 15>]().$method()),
                _ => unreachable!("pin number is 0-15"),
            }
        }
    };
}

/// 寄存器位读：按引脚号分发，返回 bool
macro_rules! pin_read_bit {
    ($rb:expr, $n:expr, $reg:ident, $fld:ident) => {
        paste::paste! {
            match $n {
                0 => $rb.$reg().read().[<$fld 0>]().bit_is_set(),
                1 => $rb.$reg().read().[<$fld 1>]().bit_is_set(),
                2 => $rb.$reg().read().[<$fld 2>]().bit_is_set(),
                3 => $rb.$reg().read().[<$fld 3>]().bit_is_set(),
                4 => $rb.$reg().read().[<$fld 4>]().bit_is_set(),
                5 => $rb.$reg().read().[<$fld 5>]().bit_is_set(),
                6 => $rb.$reg().read().[<$fld 6>]().bit_is_set(),
                7 => $rb.$reg().read().[<$fld 7>]().bit_is_set(),
                8 => $rb.$reg().read().[<$fld 8>]().bit_is_set(),
                9 => $rb.$reg().read().[<$fld 9>]().bit_is_set(),
                10 => $rb.$reg().read().[<$fld 10>]().bit_is_set(),
                11 => $rb.$reg().read().[<$fld 11>]().bit_is_set(),
                12 => $rb.$reg().read().[<$fld 12>]().bit_is_set(),
                13 => $rb.$reg().read().[<$fld 13>]().bit_is_set(),
                14 => $rb.$reg().read().[<$fld 14>]().bit_is_set(),
                15 => $rb.$reg().read().[<$fld 15>]().bit_is_set(),
                _ => unreachable!("pin number is 0-15"),
            }
        }
    };
}

/// 持有端口寄存器块借用 + 引脚号。独占性由 PAC `Peripherals::take()` 的
/// 单次语义间接保证；同端口多引脚共享寄存器块引用（读改写均经硬件
/// 原子性寄存器 BOP/BC/TG 或 PAC modify，骨架阶段可接受）。
pub struct Pin<'a> {
    rb: &'a gpioc::RegisterBlock,
    n: u8,
}

impl<'a> Pin<'a> {
    /// 输入模式（复位默认态，显式构造以自证时钟已开）
    pub fn input(rb: &'a gpioc::RegisterBlock, n: u8) -> Self {
        let mut pin = Self { rb, n };
        pin.set_mode(PinMode::Input);
        pin
    }

    /// 输出模式
    pub fn output(rb: &'a gpioc::RegisterBlock, n: u8) -> Self {
        let mut pin = Self { rb, n };
        pin.set_mode(PinMode::Output);
        pin
    }

    /// 复用功能模式（UART/SPI/CAN 等外设引脚），同时写 AF 编号
    pub fn alternate(rb: &'a gpioc::RegisterBlock, n: u8, af: u8) -> Self {
        let mut pin = Self { rb, n };
        pin.set_mode(PinMode::Alternate);
        pin.set_af(af);
        pin
    }

    /// 模拟模式（ADC/DAC）
    pub fn analog(rb: &'a gpioc::RegisterBlock, n: u8) -> Self {
        let mut pin = Self { rb, n };
        pin.set_mode(PinMode::Analog);
        pin
    }

    pub fn pin_number(&self) -> u8 {
        self.n
    }

    /// 设置复用功能编号（AFSEL0/AFSEL1 的 SELx 4bit 字段，0-15）
    /// AF8=UART6/7、AF7=USART0/1/2（GD32F470 手册 AF 表）
    pub fn set_af(&mut self, af: u8) {
        match self.n {
            0 => self.rb.afsel0().modify(|_, w| w.sel0().set(af)),
            1 => self.rb.afsel0().modify(|_, w| w.sel1().set(af)),
            2 => self.rb.afsel0().modify(|_, w| w.sel2().set(af)),
            3 => self.rb.afsel0().modify(|_, w| w.sel3().set(af)),
            4 => self.rb.afsel0().modify(|_, w| w.sel4().set(af)),
            5 => self.rb.afsel0().modify(|_, w| w.sel5().set(af)),
            6 => self.rb.afsel0().modify(|_, w| w.sel6().set(af)),
            7 => self.rb.afsel0().modify(|_, w| w.sel7().set(af)),
            8 => self.rb.afsel1().modify(|_, w| w.sel8().set(af)),
            9 => self.rb.afsel1().modify(|_, w| w.sel9().set(af)),
            10 => self.rb.afsel1().modify(|_, w| w.sel10().set(af)),
            11 => self.rb.afsel1().modify(|_, w| w.sel11().set(af)),
            12 => self.rb.afsel1().modify(|_, w| w.sel12().set(af)),
            13 => self.rb.afsel1().modify(|_, w| w.sel13().set(af)),
            14 => self.rb.afsel1().modify(|_, w| w.sel14().set(af)),
            15 => self.rb.afsel1().modify(|_, w| w.sel15().set(af)),
            _ => unreachable!("pin number is 0-15"),
        }; // svd2rust 0.37 modify() 返回 u32，语句位置丢弃
    }

    /// 切换工作模式
    pub fn set_mode(&mut self, mode: PinMode) {
        match mode {
            PinMode::Input => pin_modify!(self.rb, self.n, ctl, |w| ctl.input()),
            PinMode::Output => pin_modify!(self.rb, self.n, ctl, |w| ctl.output()),
            PinMode::Alternate => pin_modify!(self.rb, self.n, ctl, |w| ctl.alternate()),
            PinMode::Analog => pin_modify!(self.rb, self.n, ctl, |w| ctl.analog()),
        }; // svd2rust 0.37 modify() 返回 u32（写入值），语句位置丢弃
    }

    /// 输出置高（经 BOP 置位寄存器，硬件原子写）
    pub fn set_high(&mut self) {
        pin_write!(self.rb, self.n, bop, |w| bop.set_bit());
    }

    /// 输出置低（经 BC 复位寄存器，硬件原子写）
    pub fn set_low(&mut self) {
        pin_write!(self.rb, self.n, bc, |w| cr.set_bit());
    }

    /// 翻转输出（经 TG 翻转寄存器，硬件原子写，免读改写）
    pub fn toggle(&mut self) {
        pin_write!(self.rb, self.n, tg, |w| tg.set_bit());
    }

    /// 当前输出锁存电平（OCTL）
    pub fn output_level(&self) -> bool {
        pin_read_bit!(self.rb, self.n, octl, octl)
    }

    /// 输入电平（ISTAT）
    pub fn input_level(&self) -> bool {
        pin_read_bit!(self.rb, self.n, istat, istat)
    }
}

impl<'a> ErrorType for Pin<'a> {
    type Error = Infallible;
}

impl<'a> OutputPin for Pin<'a> {
    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.set_high();
        Ok(())
    }

    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.set_low();
        Ok(())
    }
}

impl<'a> InputPin for Pin<'a> {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(self.input_level())
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(!self.input_level())
    }
}
