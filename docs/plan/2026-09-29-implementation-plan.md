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

## 6. 执行记录

### 阶段 0（2026-09-29 完成，main 分支）
- git 环境 + 国内源构建容器（apt=USTC/rust=rsproxy/pip=清华，probe-rs 0.32 vendored）
- 裸机 blink（手写寄存器）板上验证；7 份官方 datasheet 入库
- 踩坑记录：GD32F470 主 SRAM=128K@0x20000000（非 512K 连续）；RCU 基址
  0x40023800（手算错两轮，Fault 寄存器 BFAR 定位）

### 阶段 1（2026-09-29 完成，feat/stage1-async-time-driver 分支）
- PAC：svd2rust 0.37 从 DFP SVD 生成（tools/gen-pac.sh 可复现；patch-svd.py
  清洗非法 access/控制字符 + 补 GPIO CTL 枚举 + TIMER 数值字段 writeConstraint）
- blink-pac：纯安全 API 固件（#![deny(unsafe_code)]），寄存器级验证
- embassy-gd32 骨架：gpio（表驱动宏+embedded-hal 1.0）+ rcc + interrupt
  typelevel 基础设施 + time driver
- 异步化：TIMER1@1MHz time driver（32 位单比较闹钟）+ blink-async 板上验证
- 关键实测：TIMER1 是 32 位计数器；PSC 影子寄存器需 UG 事件锁存；
  外设时钟未开时寄存器写丢弃；svd2rust 0.37 不生成 pub mod interrupt
  （cortex-m-rt #[interrupt] 需 HAL 提供作用域模块）
- 约束执行：固件零 unsafe；HAL 仅 2 处注释过的 unsafe 收敛点

### 阶段 2（2026-09-29 完成，feat/stage2-usart 分支）
- 串口：UART6 (PC_RS232_1, PE7/PE8 AF8) 轮询回显——PC 环回 PASS（dc484f8）
- RS485_1：USART1 缓冲回显——PC 环回 PASS（c44197c）。实测定案：
  USART1 在 PD5/PD6（原理图网络名 UART4_TX/RX 系设计者笔误）、
  DIR1=PD4 低=发送（板级反相）、隔离光耦需 4 字节时间换向窗
- SPI Flash：SPI3 @ PE2/5/6 + CS=PE4——JEDEC EF 40 18 = W25Q128（51af182，
  原理图网络名 W25Q256 系预留误标）+ 读写擦验收 FLASH_RW PASS（251f190）
- bxCAN：CAN0 内部回环自检 3/3 帧匹配 PASS（6253511）。实测定案：
  bxCAN 复位自动进睡眠（先清 SLPWMOD 再 IWMOD）；F0DATA0/1 上电随机值
  必须显式清零（GD32 与 ST bxCAN 行为差异）
- tools/patch-svd.py 定稿：access/name 清洗 + GPIO CTL 枚举 + 全值域
  writeConstraint（TIMER/UART/SPI/CAN/GPIO AFSEL），Safe writer 全覆盖

### 阶段 2 续（2026-09-29，feat/stage2-usart / feat/stage2-dms-io 分支）
- CPLD 协议驱动：SPI2 + V1.0 协议（0x55AA 应答全双工内嵌）——CPLD_TEST PASS
  （uart_mux 写读往返 MATCH、EXIO 写应答 OK；688f7d8）
- DMS IO：OUT×8 走位 + IN×8 状态流 + CTRL×2 翻转——固件级 RUNNING（23e2207，
  电气回读验收需外部 jumper/meter）
- 踩坑沉淀：UART6 引脚配置遗漏导致 console 静默（flash-identify/dms-io-test
  两度踩中，已加注释固化）；CPLD 应答为全双工内嵌时序（非命令后附加字节）

### SRAM 地图定案（2026-09-29，openocd 逐段探针板上实证）
- 主 SRAM 448KB 连续 @ 0x20000000（SRAM0 112K + SRAM1 16K + SRAM2 64K +
  ADDSRAM 256K；0x20070000 起总线错误实测边界）
- TCMSRAM 64KB @ 0x10000000：仅内核 DBUS（DMA 禁入），适合关键任务栈/
  executor 热结构/RTT 缓冲；DMA 缓冲禁止放置
- 宣传 512K = 448K 主 SRAM + 64K TCM（社区帖"512K 含备份区 64K"即此意）
- memory.x 已定案 448K；ram-probe 全量测试进行中
- 优化待办：TCP 服务器稳定后，将中断栈 + embassy executor 热结构迁 TCM

### 阶段 4 前置（2026-09-29 完成，feat/stage4-protocol-uart 分支）
- UART6 串行控制协议（文本行协议，传输层可替换为 TCP）：
  - v1：ping/cpld mux/out/in/ctrl——CONTROL PASS（c682e50）
  - v2：can0 send 接入——CONTROL_V2 PASS（9e50905，CAN 总线 ACK 实证 TJA1050 活动）
  - v3：flash jedec/read + can0 recv——V3 CHECK PASS（cc8f80e）
- 修复合集：out/ctrl 3-token 条件、can0 双分支合并（recv 2-token 命令被
  nt>=3 守卫挡住的 token 计数 bug）、read_byte 手册推荐流（STAT0→DATA）
- 板上定案数据：W25Q128 addr0 遗留 "carrier_" 数据（carrier-box 历史）；
  can0 send OK sent（TJA1050 活动、总线 ACK 存在）

### 阶段 2g（2026-09-29 完成，feat/stage2g-usart-async 分支）
- USART6 中断驱动接收环 + embassy 异步任务回显——FULL-PASS（f6b058d）
- 根因：main 未调 init_time_driver()，TIMER1 未初始化 → Ticker 闹钟永不
  触发 → 任务卡死首次 await（banner 正常但心跳/回显全无）
- 修复后实测：心跳 'A' 500ms + RX 中断环回显 AXA/AYZA（与心跳交织）
- 流程教训：改 memory.x 后必须 touch 强制重链（增量构建不重链向量表，
  曾致"烧录成功"实为旧 ELF）——已固化到 memory

### 阶段 2g 终版（2026-09-29，789ebc6）
- UART6 向量所有权归 HAL（usart.rs 独占 #[interrupt] fn UART6 + 环），
  固件消费侧零 unsafe（uart6_ring_pop）；修复双 crate 重复定义导致的
  bitcode 链接失败（usart-async-test 唯一失败 bin 的根因）
- 快速 256B 连发验收：两次独立运行均 256/256 完美回显（FAST_BURST PASS；
  之前 255/256 为探针字节串扰的测试时序问题，非固件缺陷）
- 流程教训：改 memory.x 后 touch 强制重链；openocd cfg 结尾 reset run；
  构建统一用户（root/ubuntu 交替致 target 权限混乱需 --user root 清理）

### 阶段 5 前置（2026-09-29 完成，feat/stage5-bootloader 分支）
- bootloader 跳转链验证——LED 慢闪经 app@0x08008000 实证（d2b1d6d）：
  - bootloader @0x08000000（32K 区）：合法性检查（SP∈448K SRAM 界 + reset
    thumb 位）-> VTOR 重定向 -> MSP 重载 -> 跳转（cortex-m-rt 官方序列）
  - app-at-offset @0x08008000：独立 workspace，build.rs 提供 memory.x 搜索
    路径（根配置 rustflags 叠加致 -Tlink.x 双份 -> FLASH 重复定义的修复）
  - delay 修复：spin_loop 循环被 O1 优化空转 -> volatile 递减确定性 delay
  - 烧录：openocd 双镜像（bl@0x08000000 + app@0x08008000，各自 verify）
- 待续：W25Q 升级通道（控制协议写镜像到 W25Q -> bootloader 校验签名/
  CRC -> 搬运到应用区 -> 跳转）
