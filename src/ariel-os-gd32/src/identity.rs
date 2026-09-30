//! 芯片标识（官方契约：identity 模块——DeviceId 语义；GD32F470 的
//! 96-bit UID @ 0x1FFF F710（GD32F4xx 用户手册 §2.7，与 ST 例外布局
//! 不同——GD32 自有 UID 基址）
//!
//! 官方契约对照：ariel-os-stm32/src/identity.rs 导出
//! `DeviceId([u8; N])` 类型 + `device_id()` 读取函数

/// 芯片唯一 ID（96-bit = 12 字节，GD32F4xx UID 基址 0x1FFF_F710，
/// 三个 32-bit 字按地址序拼接）
pub struct DeviceId(pub [u8; 12]);

/// UID 寄存器基址（GD32F4xx 用户手册定案）
const UID_BASE: u32 = 0x1FFF_F710;

/// 读取芯片唯一 ID
pub fn device_id() -> DeviceId {
    let mut id = DeviceId([0u8; 12]);
    for (i, word) in [(0u32), (4), (8)].iter().enumerate() {
        // unsafe 收敛点：UID 为只读物理常驻地址（SVD 外手册定案），
        // volatile 读无别名写
        let v = unsafe { core::ptr::read_volatile((UID_BASE + word) as *const u32) };
        id.0[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    id
}
