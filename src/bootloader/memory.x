/* bootloader 独立内存布局：FLASH 前 32K 归 bootloader，应用从 0x08008000 起
 * （主 SRAM 448K 定案：栈顶 0x20030000，192K 可靠区内） */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 32K
  RAM   : ORIGIN = 0x20000000, LENGTH = 192K
}
