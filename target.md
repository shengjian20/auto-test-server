# 资源

| 序号 | 资源路径 | 资源类型 |
|-----|---------|---------|
| 1 | ${workspace}/doc/pcb-00357-carrierctrl-eth_v1.pdf | 设备电路原理图 |
| 2 | ${workspace}/doc/载具控制器FPGA与MCU通讯协议.md | MCU与CPLD通信协议 |
| 3 | Bus 001 Device 115: ID 04d8:0053 Microchip Technology, Inc. Chuangxin Tech USBCAN/CANalyst-II | USB设备 |
| 4 | Bus 001 Device 079: ID 067b:23a3 Prolific Technology, Inc. ATEN Serial Bridge | USB设备 |
| 5 | Bus 001 Device 120: ID 1a86:7523 QinHeng Electronics CH340 serial converter | USB设备 |
| 6 | Bus 001 Device 123: ID 0d28:0204 NXP ARM mbed | USB设备 |

# 电气拓扑

`Bus 001 Device 115: ID 04d8:0053 Microchip Technology, Inc. Chuangxin Tech USBCAN/CANalyst-II` 的 `CH1` 与 `MCU` 的 `CAN1` [内部寄存器的CAN0] 相连
`Bus 001 Device 079: ID 067b:23a3 Prolific Technology, Inc. ATEN Serial Bridge` 与 `MCU` 的 `PC_RS232_1` [内部寄存器的uart6] 相连
`Bus 001 Device 120: ID 1a86:7523 QinHeng Electronics CH340 serial converter` 与 `MCU` 的 `RS485_1` [内部寄存器的uart1] 相连
`Bus 001 Device 123: ID 0d28:0204 NXP ARM mbed` 与 `MCU` 的 `SWD` [调试总线] 相连
`LAN8720A` 与 `MCU` 的 `ETH` [EMAC] 相连

# 软件拓扑

`Bus 001 Device 079: ID 067b:23a3 Prolific Technology, Inc. ATEN Serial Bridge` 对应 `PC机` 的 `/dev/ttyUSB0`
`Bus 001 Device 120: ID 1a86:7523 QinHeng Electronics CH340 serial converter` 对应 `PC机` 的 `/dev/ttyUSB1`
`Bus 001 Device 123: ID 0d28:0204 NXP ARM mbed` 对应 `PC机` 的 `/dev/ttyACM0` [使用SeggerRTT技术产生的USB虚拟串口]

# 可参考设计

| 序号 | 资源路径 | 资源类型 |
|-----|---------|---------|
| 1 | ${workspace}/../carrier-box/ | 已实现外设功能工程 |
| 2 | ${workspace}/../can_service/ | USBCAN驱动实现 |

# 设计目标

 - 搭建git环境,每一个流程都需要使用git记录
 - 上传github记录云端
 - 从零搭建目标容器,全程在目标容器中编译构建调试
 - 使用Ubuntu镜像
 - 完成开发编译调试全流程的打通
 - 使用rust作为开发语言
 - 为GD32F470适配rust的embedded hal
 - 使用Ariel OS
 - 实现bootloader升级
 - flash驱动实现
 - 完成所有PCB原理图中为外设的驱动
 - 适配网络协议栈
 - 将所有外设都抽象
 - 基于网络协议栈实现TCP服务器
 - 在PC端能够通过TCP服务器实现对设备外设的控制 外设范围[232串口(除uart6控制台)、485串口、can1/can2、IO(包括CPLD/FPGA上的IO)、串口切换功能(CPLD/FPGA提供的串口切换)]
 - 设计一个SKILL & MCP实现AGENT对于该自动测试服务器的控制权

# 片外FLASH用途设计评估
评估一下这些的方案可行性  
1、W25Q256(flash)搭载文件系统  
2、W25Q256(flash)充当本地数据库  
3、W25Q256(flash)充当固件存放地址  

# 设计约束

 - 源码让在src目录下
 - 源码需要解耦设计,模块化设计
 - 产生的过程文档需要记录在一个目录下,且需要分类放好
 - 获取/产生的资料需要收集到doc目录下
 - 不允许对宿主机产生破坏性修改