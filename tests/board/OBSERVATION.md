# 串口观测方法论（重要——避免"静默"假象）

## 核心规则：单容器内完成"复位+读取"

DAPLink 是 USB 复合设备（CDC 串口 + HID 调试）。**跨容器**分别跑
串口读取与 probe-rs/openocd 复位时，调试容器打开 USB 会短暂断开
CDC 流——串口侧表现为"静默"（观测假象）。

正确形态（unified_watch.py）：单容器内
1. pyserial 先开并监听
2. 同容器内 openocd/probe-rs 复位
3. 同容器 pyserial 继续读

## 附带坑位

- banner 只在上电/复位瞬间发一次：读取窗必须先于复位打开
- 长静默固件（搬运流程等）：进度打点（bootloader 8KB/行）
- 观测到"direct write 无响应"时先用 unified_watch 抓完整流再定性
