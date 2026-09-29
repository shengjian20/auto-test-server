# 载具控制器FPGA与MCU通讯协议

| 版本 | 时间 | 变更记录 |
| --- | --- | --- |
| V1.0 | 2023/04/21 | 初稿 |
|  |  |  |

FPGA IO寄存器（SPI接口）定义 

发送帧：

| byte0 | byte1 |
| --- | --- |
| cmd: (1/2/3/4) << 12<br>详细参考REG\_in | write\_data:<br>cmd 1: exgpo 设置cpld寄存器的output<br>cmd 2: 无写操作，仅读取<br>cmd 3: exuart 设置cpld寄存器的uart\_mux<br>cmd 4: 无写操作，仅读取 |

接收帧：

| byte0 | byte1 |
| --- | --- |
| cmd\_res: 固定0x55AA | read\_data:<br>cmd 1: exgpi 读取cpld寄存器的gpi状态<br>cmd 2: exgpi 读取cpld寄存器的gpi状态<br>cmd 3: exuart 读取cpld寄存器的uart\_mux状态<br>cmd 4: exuart 读取cpld寄存器的uart\_mux状态 |

其中I/O方向针对FPGA

| Reg | 位 | 描述 |
| --- | --- | --- |
| Reg\_in\[15:0\] | \[15:12\]：控制子 | reg\_in\[15:12\] = 4‘d1 时，<br>dout = Reg\_in\[5:0\]<br>reg\_in\[15:12\] = 4‘d1 或 4'd2时，<br>Reg\_out = {4'd0, din}<br>reg\_in\[15:12\] = 4‘d3 时，<br>uart\_sel=Reg\_in\[2:0\]<br>reg\_in\[15:12\] = 4‘d3 或 4'd4时，<br>Reg\_out = {13'd0, uart\_sel} |

| uart\_sel\[2:0\] | 0 | extup\_uart = mcu\_uart |
| --- | --- | --- |
|  | 1 | extup\_uart = ext\_uart0 |
|  | 2 | extup\_uart = ext\_uart1 |
|  | 3 | extup\_uart = ext\_uart2 |
|  | 4 | extup\_uart = ext\_uart3 |
|  | 5 | extup\_uart = ext\_uart4 |
|  | 6 | extup\_uart = ext\_uart5 |
|  | default | extup\_uart = mcu\_uart |