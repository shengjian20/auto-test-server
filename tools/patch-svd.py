#!/usr/bin/env python3
"""DFP SVD 清洗：修复 svd2rust 无法解析/无法生成安全 API 的缺陷。

GigaDevice DFP 3.5.0 的已知问题与本脚本的处理：
1. 非法 access 枚举（write/read -> write-only/read-only），5 处
2. name 元素中的控制字符/空白（如 RTC_T<tab>amper）
3. GPIO CTLx 补 enumeratedValues（DFP 缺失；无枚举时 svd2rust 0.37 只出
   unsafe bits()，补齐后生成 input()/output()/alternate()/analog() 安全变体）
4. 全值域数值字段补 writeConstraint range（svd2rust 0.37 对无约束字段只生成
   unsafe bits()；有约束则生成 Safety=Safe 的 .set() 安全方法）。字段语义依据
   GD32F470 用户手册（均为无保留位的全宽/全值域字段）：
   - TIMER1: PSC/CAR/CNT/CH0CV/CH1CV
   - UART: BAUD/DATA；GPIO: AFSEL0/1 的 SELx
   - SPI0(derivedFrom 覆盖 SPI1-4): DATA 全宽、CTL0.PSC 3bit 分频档
   - CAN0/1: TMI/TMP/RFIFOM*/RFIFOMP* 的 ID/DLC/TS 字段，TMDATA/RFIFOMDATA
     的 DB0-7 数据字节
用法: python3 patch-svd.py <raw.svd> <patched.svd>
"""
import sys
import xml.etree.ElementTree as ET
import re

src, dst = sys.argv[1], sys.argv[2]
FIX_ACCESS = {"write": "write-only", "read": "read-only"}

GPIO_MODES = [
    ("Input", "Floating/pull input mode (reset state)", 0),
    ("Output", "General purpose output mode", 1),
    ("Alternate", "Alternate function mode", 2),
    ("Analog", "Analog mode", 3),
]

TIMER_FULLRANGE = ("PSC", "CAR", "CNT", "CH0CV", "CH1CV")
UART_FULLRANGE = ("BAUD", "DATA")
GPIO_AFSEL = ("AFSEL0", "AFSEL1")
SPI_RANGE = {"DATA": None, "CTL0": "PSC"}  # 寄存器名 -> 仅该字段(None=全部)
CAN_ID_FIELDS = ("SFID_EFID", "EFID", "DLENC", "TS")
CAN_DATA_FIELDS = ("DB0", "DB1", "DB2", "DB3", "DB4", "DB5", "DB6", "DB7")
CAN_ID_REGS = ("TMI0", "TMI1", "TMI2", "TMP0", "TMP1", "TMP2",
               "RFIFOMI0", "RFIFOMI1", "RFIFOMP0", "RFIFOMP1", "RFIFOMP2")
# ENET MAC PHY MDIO 控制（手册 ENET 章节）：CLR 3bit HCLK 分频范围 0-7、
# PA 5bit PHY 地址 0-31、PR 5bit PHY 寄存器 0-31——均为全值域
ENET_PHY_FULLRANGE = ("CLR", "PA", "PR")
ENET_PHY_REG = "MAC_PHY_CTL"
# MAC_PHY_DATA 的 PD 字段（16 位 MDIO 数据全值域）
ENET_PHY_DATA_REG = "MAC_PHY_DATA"
# BT 位时序字段：SJW 2b/BS1 4b/BS2 3b/BAUDPSC 10b 全部为无保留位全值域
# （手册位时序章节：字段值为 N-1 编码，全范围合法）
CAN_BT_FIELDS = ("SJW", "BS1", "BS2", "BAUDPSC")
CAN_DATA_REGS = ("TMDATA00", "TMDATA10", "TMDATA01", "TMDATA11",
                 "TMDATA02", "TMDATA12",
                 "RFIFOMDATA00", "RFIFOMDATA10",
                 "RFIFOMDATA01", "RFIFOMDATA11")

tree = ET.parse(src)
root = tree.getroot()
n_acc = n_name = n_enum = 0

# 1+2: access 枚举与 name 控制字符
for acc in root.iter("access"):
    if acc.text in FIX_ACCESS:
        acc.text = FIX_ACCESS[acc.text]
        n_acc += 1
for name in root.iter("name"):
    if name.text and (name.text != name.text.strip() or re.search(r"[\t\n\r]", name.text)):
        name.text = re.sub(r"[\t\n\r]", "", name.text).strip()
        n_name += 1


def add_range_constraint(field):
    global n_enum
    if field.find("writeConstraint") is not None:
        return
    fw = int(field.findtext("bitWidth") or "0")
    if fw == 0:
        return
    wc = ET.SubElement(field, "writeConstraint")
    rng = ET.SubElement(wc, "range")
    ET.SubElement(rng, "minimum").text = "0"
    ET.SubElement(rng, "maximum").text = str((1 << fw) - 1)
    n_enum += 1


for peri in root.iter("peripheral"):
    pname = peri.findtext("name") or ""

    # 3: GPIO CTL 枚举 + AFSEL0/1 SELx 全值域约束
    if pname.startswith("GPIO"):
        for reg in peri.iter("register"):
            rn = reg.findtext("name") or ""
            if rn == "CTL":
                for field in reg.iter("field"):
                    fname = field.findtext("name") or ""
                    if not re.fullmatch(r"CTL\d+", fname) or field.find("enumeratedValues") is not None:
                        continue
                    evs = ET.SubElement(field, "enumeratedValues")
                    for vname, vdesc, val in GPIO_MODES:
                        ev = ET.SubElement(evs, "enumeratedValue")
                        ET.SubElement(ev, "name").text = vname
                        ET.SubElement(ev, "description").text = vdesc
                        ET.SubElement(ev, "value").text = str(val)
                    n_enum += 1
            elif rn in GPIO_AFSEL:
                # SELx 4bit：AF 编号 0-15 全部为合法值（手册 AF 表）
                for field in reg.iter("field"):
                    if (field.findtext("name") or "").startswith("SEL"):
                        add_range_constraint(field)
        continue

    # 4a: TIMER1 全值域
    if pname == "TIMER1":
        for reg in peri.iter("register"):
            if (reg.findtext("name") or "") not in TIMER_FULLRANGE:
                continue
            for field in reg.iter("field"):
                add_range_constraint(field)
        continue

    # 4b: UART 全值域
    if pname.startswith("USART") or pname.startswith("UART"):
        for reg in peri.iter("register"):
            if (reg.findtext("name") or "") in UART_FULLRANGE:
                for field in reg.iter("field"):
                    add_range_constraint(field)
        continue

    # 4c: SPI0（覆盖 SPI1-4 derivedFrom）DATA 全宽 + CTL0.PSC
    if pname == "SPI0":
        for reg in peri.iter("register"):
            rn = reg.findtext("name") or ""
            if rn == "DATA":
                for field in reg.iter("field"):
                    add_range_constraint(field)
            elif rn == "CTL0":
                for field in reg.iter("field"):
                    if (field.findtext("name") or "") == "PSC":
                        add_range_constraint(field)
        continue

    # 4d2: ENET MAC PHY_CTL 全值域 + MAC_PHY_DATA PD 全宽
    if pname == "ENET_MAC":
        for reg in peri.iter("register"):
            rn = reg.findtext("name") or ""
            if rn == ENET_PHY_REG:
                for field in reg.iter("field"):
                    if (field.findtext("name") or "") in ENET_PHY_FULLRANGE:
                        add_range_constraint(field)
            elif rn == ENET_PHY_DATA_REG:
                for field in reg.iter("field"):
                    if (field.findtext("name") or "") == "PD":
                        add_range_constraint(field)
        continue

    # 4d: CAN0/1 ID/DLC/TS 与 DB0-7
    if pname.startswith("CAN"):
        for reg in peri.iter("register"):
            rn = reg.findtext("name") or ""
            if rn == "BT":
                allowed = CAN_BT_FIELDS
            elif rn in CAN_ID_REGS:
                allowed = CAN_ID_FIELDS
            elif rn in CAN_DATA_REGS:
                allowed = CAN_DATA_FIELDS
            else:
                continue
            for field in reg.iter("field"):
                if (field.findtext("name") or "") in allowed:
                    add_range_constraint(field)

tree.write(dst, encoding="utf-8", xml_declaration=True)
print(f"patched: {n_acc} access, {n_name} names, {n_enum} field constraints -> {dst}")
