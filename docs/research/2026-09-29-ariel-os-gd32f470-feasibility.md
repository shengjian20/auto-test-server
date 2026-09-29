# GD32F470 × Ariel OS 技术选型调查报告

> 日期：2026-09-29
> 调查方式：Ariel OS 源码 clone（commit 31d19510d2670e1f57d324c14f9da62971acfc1b）+ crates.io API + GitHub API 全量搜索 + 寄存器级文献对比
> 结论有效性：所有链接均指向 permalink 或 PR，可复核

## 一、核心结论（TL;DR）

1. **Ariel OS 不支持 GD32**，且 STM32F4 家族内只支持 F401RE/F411RE，连 F407 都没有。
2. **Ariel OS 强绑定 Embassy HAL**——新增 GD32 支持的前提是"存在 GD32 的 Embassy HAL"，而这个 HAL 不存在。
3. **gd32f4xx-hal crate 在 crates.io 上不存在**（空集，不是"不活跃"）。gd32f4 PAC 停在 2022 年的 0.1.0-alpha.1。
4. **probe-rs ≥ v0.32.0 已官方支持 GD32F470 烧录调试**（PR #3883）——基础设施层无忧。
5. GD32F470 对标 STM32F429（240MHz），非 F407；GPIO/UART/SPI/bxCAN 寄存器级兼容，**Flash/RCC/USB/ETH 四大件有实质差异**。

## 二、五个问题的证据

### Q1: Ariel OS 官方支持范围

- 支持矩阵（https://ariel-os.github.io/ariel-os/dev/docs/book/hardware-functionality-support.html）：
  nRF52/53/91、RP2040/RP2350、ESP32-C3/C6/S2/S3、STM32（C0/F0/F3/F4/F7/G4/H7/L4/U0/U5/WB/WBA/WL）。**无 GD32**。
- 源码 laze-project.yml L647-657：stm32f4 家族 context 仅 `stm32f401re`、`stm32f411re`。
  https://github.com/ariel-os/ariel-os/blob/31d19510d2670e1f57d324c14f9da62971acfc1b/laze-project.yml#L647-L657
- **Ariel OS 的 Ethernet 功能只在 STM32H7 上标记 ✅，F4 全系列无 Ethernet 支持**。
  Ariel OS 的 net 功能依赖以太网或 USB RNDIS——两者在 GD32 上都是高风险项（见 Q4）。

### Q2: Ariel OS 外部芯片支持架构

官方文档（book/src/adding-board-support.md）声明：

> "Ariel OS supports most HALs that Embassy supports, including esp-hal, nrf, rp, and stm32... The steps to add support for another Embassy supported HAL are: src/ariel-os-hal 的 Cargo.toml 加依赖 + lib.rs 加 dispatch + 创建一个全新的 Ariel OS HAL crate"

三条扩展路径成本：

| 路径 | 成本 |
|---|---|
| 同家族加 board（SBD yaml） | 天级 |
| 同家族加 MCU（laze context + rcc.rs 条目 + support_matrix + ariel-chips.yaml） | 周级 |
| 加新 HAL 家族（GD32 属于此级） | **月~年级，且前提（Embassy GD32 HAL）不存在** |

ariel-os-stm32 直接依赖 embassy-stm32 0.4 + stm32-metapac 18.0，每个芯片需要专门的 RCC 时钟配置条目（src/ariel-os-stm32/src/rcc.rs，每芯片一个 `#[cfg(context = ...)]` 块）。

### Q3: gd32f4xx-hal 现状

- **crates.io 上不存在 gd32f4xx-hal**（API 查询返回 None）。
- ~~`gd32f4` PAC：https://crates.io/crates/gd32f4 → 0.1.0-alpha.1（2022-02-05），从未正式发布。~~
  **【2026-09-29 更正】** crates.io 发布版确实停在 alpha，但 **gd32-rs-nightlies 渠道持续活跃**：
  `gd32f4 v0.9.2`（2026-09-11 构建，https://github.com/gd32-rust/gd32-rs-nightlies ，随 gd32-rs main
  分支自动重建），feature `gd32f425`，支持 `rt`/`critical-section`。part table（gd32_part_table.yaml）
  的 gd32f4 家族仅列 GD32F425 一个成员，但寄存器模型覆盖 F4 代（F425/427/450/470 同代同源），
  F470 可直接使用，必要时以 DFP SVD 补 F470 特有 part。
- **本地 DFP SVD 验证（carrier-box/board/openocd/GigaDevice.GD32F4xx_DFP.3.5.0.pack）**：
  1.97MB 全功能 SVD，ENET_MAC(21 regs)/ENET_DMA/ENET_PTP/ENET_MSC 建模齐全；
  RCU 的 CK48MSEL/PLL48MSEL/IRC48MEN、FMC 的 WS/WSEN/OBCTL0/1/PECFG 等 GD 专有寄存器全部建模；
  88 中断号完整（ENET=61/62）；附 512KB/1MB/2MB/3MB 四档 Flash 编程算法 FLM，覆盖 F470 3MB 机型。
  quality 结论：可作为 svd2rust 备选源与驱动开发的寄存器级权威对照。
- gd32-rust 唯一活跃 HAL 是 gd32f1x0-hal（2026-09 仍有提交）——与 F4 无关。
- F4 方向唯一活跃尝试：RationalAsh
  - https://github.com/RationalAsh/embassy-gd32 （自述 WIP，仅 time driver，无 GPIO/UART/ETH/USB）
  - https://github.com/RationalAsh/gd32f4pac

### Q4: GD32F470 vs STM32F407 兼容性

GD32F470 实际对标 STM32F429：240MHz / 3MB flash / 768KB SRAM / FS+HS OTG
（vs F407：168MHz / 1MB / 192KB / 仅 FS）。
来源：https://www.gigadevice.com/product/mcu/high-performance-mcus/gd32f4xx-series

寄存器级差异（来源：elmagnifico 对 F450 的对比，F470 同族适用：
http://github.elmagnifico.tech/2021/06/16/STM32F429-GD32F450-Replace/ ）：

| 模块 | 差异 | 杀伤力 |
|---|---|---|
| Flash | GD 零等待设计，FLASH_ACR→FMC_WS 完全不同，多 FMC_WSEN，bank 不对称 | embassy flash/storage 驱动必不工作 |
| RCC | RCU_CFG0 vs RCC_CFGR；240MHz；独立 CK48M 选择（PLL48MSEL）+ IRC48M + CTC；APB1=60M/APB2=120M | embassy-stm32 按硅片假设算分频，需 fork 修改 |
| USB OTG | GCCFG bit19/18 必须置位否则不工作；RX FIFO 1024 vs 256；DSTAT 挂起极性相反 | embassy-stm32 USB 驱动直接跑必挂 |
| ETH MAC | 同 Synopsys IP，大体同源，但时钟路径多 ENET_PHY_SEL、PLLSAI 依赖 | 中风险，无先例验证 |
| GPIO/UART/SPI/bxCAN | 寄存器地址与行为级兼容（C 生态大量直接套用 ST 库先例） | ✅ 大概率免改可用 |

### Q5: 先例

- ariel-os 仓库 GD32 issue/PR：0 条（GitHub API 验证）
- embassy 仓库 GD32 issue/PR：0 条
- probe-rs PR #3883（2026-03-06 合并）：GD32F403/405/407/425/427/450/**470** 目标支持，
  GD32F425RG 硬件验证，随 v0.32.0（2026-07-22）发布
  https://github.com/probe-rs/probe-rs/pull/3883
- 无任何 "GD32F470 + Ariel OS" 或 "GD32F470 + embassy-stm32" 公开跑通案例。

## 三、方案对比

| | A: Ariel OS + 新增 GD32 支持 | B: embassy + gd32f4xx-hal | C: embassy-stm32 f407 兼容模式 |
|---|---|---|---|
| 前置条件 | GD32 Embassy HAL 不存在 | gd32f4xx-hal 不存在 | probe-rs ✅ embassy-stm32 f407 ✅ |
| 工作量 | 6-12 人月起 | 自写 HAL 3-6 人月起 | bring-up 1-3 人周；ETH +2-4 人周；USB 可能不可行 |
| ETH(LAN8720) | 同样要自己写 | 同样要自己写 | 中风险，无先例 |
| 风险 | 最高 | 高 | 中（未定义行为区） |

## 四、明确结论

1. **否决方案 A 原义**：GD32 意味着先发明 Embassy HAL 再集成 Ariel OS。且 Ariel OS 核心卖点
   （网络只在 H7 ✅、persistent storage）在本板外设组合上恰好全是缺口——
   **Ariel OS 在 GD32F470 上退化为没有网络层的线程库**，设计目标自相矛盾。
2. **否决方案 B 原义**：gd32f4xx-hal 是事实空集。
3. **推荐方案 C**：embassy-stm32 0.4 + stm32f407 feature，fork 修 RCC（先跑 168MHz 等效频率），
   GPIO/UART/SPI/CAN 预期 1-3 人周。放弃 USB，ETH 列为独立风险攻关项。probe-rs ≥ 0.32.0 烧录调试。
4. **若 Ariel OS 是硬约束**：唯一路径是 "Ariel OS + stm32f407 context + fork embassy-stm32 打 GD32 补丁"，
   代价是无 Ethernet/无 USB/无 storage 的 Ariel OS。
   **若 ETH 不可妥协，正确决策是主控换 STM32F407/H743**——那是唯一让 Ariel OS/embassy 全功能落地的路径。

## 五、不确定项声明

- GD32F470 ENET 与 F407 MAC 寄存器差异未逐位验证（无公开对比文档），"改时钟可跑"是基于同源 IP 的推断。
- USB 判死基于 F450 代寄存器级证据，F470 未复测。
- Ariel OS 支持矩阵为 2026-09 时点快照，后续版本可能变化。
