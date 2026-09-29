/* GD32F470VGT6 memory map（GD32F4xx UM + openocd/gdb 逐段探针板上定案 2026-09-29）
 *
 * 主 SRAM 448KB 连续 @ 0x20000000（实测边界：0x20070000 起 Reserved 总线错误）：
 *   SRAM0  112KB 0x20000000-0x2001BFFF
 *   SRAM1   16KB 0x2001C000-0x2001FFFF
 *   SRAM2   64KB 0x20020000-0x2002FFFF
 *   ADDSRAM 256KB 0x20030000-0x2006FFFF
 * TCMSRAM 64KB @ 0x10000000：仅内核 DBUS（DMA 禁入），DMA 缓冲禁止放置
 *
 * 分段声明（重要）：
 *   RAM (192K, 0x20000000-0x2002FFFF) = 栈 + .data/.bss（已实测 100% 可靠区）
 *   RAM2 (256K, 0x20030000-0x2006FFFF) = 扩展数据区（大缓冲/堆）
 *   栈顶曾用 0x20070000 导致启动即死（Reserved 边界紧邻，gdb 单步实证
 *   SP 被环境拉回 0x20030000——保守起见栈留 192K 区）
 */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 1024K
  RAM   : ORIGIN = 0x20000000, LENGTH = 192K
  RAM2  : ORIGIN = 0x20030000, LENGTH = 256K
}
