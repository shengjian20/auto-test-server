#!/usr/bin/env python3
"""DFP SVD 清洗：修复 svd2rust 无法解析/生成安全 API 的缺陷。
1. 非法 access 枚举（write/read -> write-only/read-only），DFP 3.5.0 有 5 处
2. name 元素中的控制字符/空白（如 RTC_T\\tamper）
3. GPIO CTLx 字段补 enumeratedValues（DFP 缺失，无枚举时 svd2rust 0.37 只出
   unsafe bits()；补齐后生成 input()/output()/alternate()/analog() 安全变体，
   践行项目"少 unsafe"约束）"""
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

tree = ET.parse(src)
root = tree.getroot()
n_acc = n_name = n_enum = 0

for acc in root.iter("access"):
    if acc.text in FIX_ACCESS:
        acc.text = FIX_ACCESS[acc.text]
        n_acc += 1
for name in root.iter("name"):
    if name.text and (name.text != name.text.strip() or re.search(r"[\t\n\r]", name.text)):
        name.text = re.sub(r"[\t\n\r]", "", name.text).strip()
        n_name += 1

for peri in root.iter("peripheral"):
    pname = peri.findtext("name") or ""
    if not pname.startswith("GPIO"):
        continue
    for reg in peri.iter("register"):
        if (reg.findtext("name") or "") != "CTL":
            continue
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

# TIMER1 数值寄存器字段补 writeConstraint 全值域 -> svd2rust 0.37 生成
# Safety=Safe 的 FieldWriter（.set() 安全方法）。否则默认 Unsafe 只给
# unsafe bits()，违背项目"少 unsafe"约束。语义依据：PSC/CAR/CNT/CH0CV
# 均为无保留位的全宽计数字段（GD32F470 UM TIMER 章节）。
TIMER_FULLRANGE = ("PSC", "CAR", "CNT", "CH0CV", "CH1CV")
for peri in root.iter("peripheral"):
    if (peri.findtext("name") or "") != "TIMER1":
        continue
    for reg in peri.iter("register"):
        rn = reg.findtext("name") or ""
        if rn not in TIMER_FULLRANGE:
            continue
        rsize_bits = int((reg.findtext("size") or "0x20"), 16) * 8
        for field in reg.iter("field"):
            if field.find("writeConstraint") is not None:
                continue
            fw = int(field.findtext("bitWidth") or "0")
            if fw == 0:
                continue
            wc = ET.SubElement(field, "writeConstraint")
            rng = ET.SubElement(wc, "range")
            ET.SubElement(rng, "minimum").text = "0"
            ET.SubElement(rng, "maximum").text = str((1 << fw) - 1)
            n_enum += 1

tree.write(dst, encoding="utf-8", xml_declaration=True)
print(f"patched: {n_acc} access, {n_name} names, {n_enum} GPIO CTL enums -> {dst}")
