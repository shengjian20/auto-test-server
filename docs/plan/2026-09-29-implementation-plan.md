# 自动测试服务器固件 — 实施计划

> 日期：2026-09-29
> 状态：待审阅
> 选型调查依据：`docs/research/2026-09-29-ariel-os-gd32f470-feasibility.md`

## 0. 选型决议（基于调查报告 + 用户确认方向）

| 目标项 | 决议 |
|---|---|
| 开发语言 | Rust（stable + thumbv7em-none-eabihf） |
| HAL | **embassy-gd32**（PAC 用 gd32-rs 社区实现 `gd32f4` nightly；async 骨架仿 embassy-stm32；GD 寄存器惯例参考 gd32f1x0-hal）——即 target.md"为GD32F470适配rust的embedded hal"目标的实现 |
| 异步运行时 | embassy-executor + embassy-time（芯片无关，直接可用） |
| Ariel OS | **阶段 7 适配分支**：主线造的 embassy-gd32 就位后，走 Ariel OS 官方"新增 HAL 家族"路径（ariel-os-hal dispatch + ariel-os-gd32 + laze context + board yaml）；主线不被阻塞；net 集成取决于阶段 3 eth 驱动 |
| 网络栈 | smoltcp + 自研 eth 驱动（GD ENET ≈ Synopsys MAC，LAN8720A RMII） |
| 烧录/调试 | probe-rs ≥ 0.32.0（GD32F470 官方支持，用户已确认） |
| 编译环境 | Docker 容器（Ubuntu 基础镜像），全程容器内构建，USB 设备透传 |
| 版本管理 | git 全程记录，阶段完成后推送 GitHub（仓库名待用户提供） |

## 1. 已确认的硬件事实（来自原理图文本层提取）

- MCU：GD32F470VGT6（LQFP100，Cortex-M4F，1024K flash / 512K SRAM）
- PHY：LAN8720A（RMII，25MHz 晶振）
- CPLD：Lattice MachXO 系（MCU 经 SPI2: PC10/11/12 访问，nRST=PA4，uart_mux 经 PD8/9）
- SPI Flash：W25Q256（PE1-5, SPI3）+ W25Q128JVSIQ
- RS485×2：UART4(PD5/6+DIR PD4)、UART5(PE0/1)，CS48520S 隔离收发器
- RS232×3：USART0(PA9/10)、USART1(PD8?/PD9 见CPLD_TX/RX)、USART2(PB6/7)... 具体以原理图页3-4引脚表为准，SP3232EEY×3
- CAN×2：CAN0(PA11/12)、CAN1(PB5/6)，TJA1050 隔离
- DMS IO：OUT1-8、IN1-8、CTRL1-2（AQY282S 光耦/MOS 驱动）
- LED×2、SWD（DAPLink /dev/ttyACM0）
- PC 侧测试资源：USBCAN(04d8:0053, usbcan-rs 驱动可用)、ATEN 232(ttyUSB0)、CH340 485(ttyUSB1)

## 2. 仓库布局

```
auto_test_server/
├── doc/                      # 原有资料（不动）
├── docs/
│   ├── plan/                 # 本计划与阶段记录
│   └── research/             # 调查报告
├── src/                      # 设计约束：源码在此
│   ├── embassy-gd32/         # HAL crate（核心资产）
│   │   └── src/{rcc,gpio,usart,spi,can,eth,timer,exti,flash}/
│   ├── firmware/             # 主固件 bin（TCP服务器+外设服务）
│   └── bootloader/           # 阶段5
├── tools/
│   └── mcp-server/           # PC 侧 MCP/SKILL（阶段6）
├── docker/
│   └── Dockerfile            # rust-embedded 构建容器
└── target.md                 # 保留
```

## 3. 阶段划分（每阶段 = 若干 git commit，全部容器内构建）

### 阶段 0：基础设施（0.5-1 天）
- git init + .gitignore + 首次提交（含 target.md、docs/）
- docker/Dockerfile：Ubuntu + rustup(stable, thumbv7em-none-eabihf) + probe-rs 0.32 + cargo-binutils
- GitHub 远端配置（用户提供 repo URL）
- 文档收集：GD32F470 用户手册/数据手册（AF表、ENET、FMC章节）→ doc/；DFP pack 中的 SVD 提取
- **验收**：容器内 `probe-rs list` 看到 DAPLink；hello-blink 固件（临时裸 PAC）烧录成功，LED 闪烁，全程 git 记录

### 阶段 1：embassy-gd32 核心（1 周）
- PAC：**svd2rust 0.37 从 DFP SVD 生成**（用户拍板；tools/gen-pac.sh 可复现，已 vendor 进 src/pac/gd32f470，
  编译通过）。DFP SVD = carrier-box 自带 GigaDevice 官方 pack 3.5.0，ENET/GD 专有寄存器建模齐全。
  项目约束：尽量少 unsafe——PAC 用生成安全 API，HAL 公共接口安全，业务层 deny(unsafe_code)
- rcc：GD32F470 时钟树（240MHz PLL、CK48M、APB1=60M/APB2=120M），按用户手册寄存器位域实现
- gpio：含 AF 表（GD32F470 手册为准，不用 ST 表）
- timer：embassy-time driver（SysTick 或 TIM）
- exti：GPIO 中断
- **验收**：blink（异步 executor 版）+ DMS IO 全 18 路翻转示波器/输入回读

### 阶段 2：串行外设（1.5-2 周）
- usart：async + DMA（232×3 + 485×2，含 RS485 DE/RE 方向控制）
- spi：async（CPLD 寄存器协议驱动：cmd1/2 GPO/GPI、cmd3/4 uart_mux，表驱动实现协议文档 V1.0）
- W25Q256/W25Q128 驱动（基于 embedded-hal spi trait，embedded-storage 接口）
- **验收**：
  - 232 环回：PC 经 ttyUSB0 收发
  - 485 环回：PC 经 ttyUSB1 收发
  - CPLD：uart_mux 切换 + GPO/GPI 读写回读正确
  - W25Q256：JEDEC ID + 读写擦全通过
- CAN 驱动（bxCAN 寄存器级兼容，从 embassy-stm32 can 移植）：3-5 天
  - **验收**：PC 侧 usbcan-rs 经 USBCAN 设备与 CAN0/CAN1 环回收发

### 阶段 3：以太网（2-4 周，风险攻关阶段）
- eth 驱动：GD ENET MAC（对照手册逐寄存器）+ LAN8720A RMII/MIIM
- smoltcp 集成（embassy-net Device trait）
- **验收**：静态 IP + TCP echo 服务，PC 侧 nc 实测吞吐
- 风险预案：若 MAC 差异超预期，退回轮询模式先通，再补中断/DMA 路径
- 交付评估文档：target.md"片外FLASH用途设计评估"（文件系统/本地数据库/固件存放三方案可行性）

### 阶段 4：TCP 控制服务器（1 周）
- 协议设计：帧头 + JSON/payload（外设控制：232/485/can/io/uart_mux）
- 多连接管理 + 外设访问仲裁（每外设单消费者模型）
- **验收**：PC 脚本经 TCP 完成全部外设操作用例

### 阶段 5：bootloader（1-1.5 周）
- FMC flash 驱动（GD 零等待控制器，独立于 HAL 主线）
- bootloader：APP 区校验 + 跳转；升级通道 = TCP（W25Q256 存暂存固件）+ SWD 兜底
- **验收**：TCP 触发升级，断电重启后运行新固件

### 阶段 6：MCP/SKILL（3-5 天）
- PC 侧 MCP server（Rust）：封装 TCP 协议为 MCP tools（外设枚举/读/写/环回测试）
- SKILL 文档：agent 使用说明
- **验收**：agent 经 MCP 完成一次全自动外设回归

### 阶段 7：Ariel OS 适配分支（前置：阶段 1-2 完成）
- ariel-os-gd32 crate：ariel-os-hal dispatch + 本仓库 embassy-gd32 作为后端
- laze context（gd32f470）+ board yaml（auto-test-server）+ support_matrix + ariel-chips.yaml
- 线程/GPIO/UART 先行；net 集成视阶段 3 eth 驱动成熟度
- **验收**：Ariel OS 线程 blink + UART hello 在目标板运行

## 4. 关键风险与预案

| 风险 | 等级 | 预案 |
|---|---|---|
| ENET MAC 与 ST eth-v1 差异超预期 | 高 | 手册逐寄存器比对在先；轮询模式降级路径；阶段3独立可砍，不阻塞阶段0-2/4-6 |
| GD32F470 SVD 质量（GigaDevice SVD 已知粗糙） | 中 | **已解决**：本地 DFP SVD 验证质量良好（ENET 全建模、GD 专有寄存器齐全、88 中断完整）；PAC 用 gd32-rs-nightlies gd32f4 v0.9.2+，DFP SVD 作对照补盲 |
| probe-rs 容器内 USB 透传 | 低 | --privileged + /dev/bus/usb 挂载，openocd 作回退 |
| AF 表/引脚复用与 ST 不同 | 中 | 全部以 GD32F470 手册为准，原理图引脚逐一核对 |

## 5. 约束遵守

- 源码全部在 src/ 下 ✓（§2 布局）
- 过程文档在 docs/ 分类放置 ✓（plan/research）
- 资料收集在 doc/ ✓（阶段0）
- 不破坏宿主机 ✓（构建全在容器，仅 git/docker/串口设备访问）
- target.md 保留 ✓
- 每流程 git 记录 ✓（阶段=里程碑 commit）
