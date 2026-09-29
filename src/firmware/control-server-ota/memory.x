/* OTA 变体镜像 @ 0x08008000（与主变体同址互斥——bootloader 搬运目标区）
 * 主 SRAM 448K 定案：栈顶 0x20030000，192K 可靠区内 */
MEMORY
{
  FLASH : ORIGIN = 0x08008000, LENGTH = 992K
  RAM   : ORIGIN = 0x20000000, LENGTH = 192K
}
