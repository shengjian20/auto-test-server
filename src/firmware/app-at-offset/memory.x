/* 应用镜像 @ 0x08008000（bootloader 后 32K）：向量表 + 应用
 * 主 SRAM 448K 定案：栈顶 0x20030000，192K 可靠区内 */
MEMORY
{
  FLASH : ORIGIN = 0x08008000, LENGTH = 992K
  RAM   : ORIGIN = 0x20000000, LENGTH = 192K
}
