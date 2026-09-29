//! control-server lib：全外设控制协议主体（main/ota 双 bin 变体共享）。
//! lib 化原因：OTA 升级链要求"应用区镜像可被替换为另一变体"——
//! 两个 bin 链接于同一地址（0x08008000）但 banner 不同，业务逻辑
//! 单一来源（app.rs），杜绝双份漂移。
#![no_std]

pub mod app;
