#!/usr/bin/env python3
"""DFP SVD 最小修复：仅做"让 svd2rust 能跑"的必需修复。

GigaDevice DFP 3.5.0 的两个 svd2rust 0.37 硬伤：
1. 非法 access 枚举（"write"/"read" 不是 SVD 合法值），5 处
2. name 元素中的控制字符（如 RTC_T<tab>amper）

禁止任何语义增强（enumeratedValues/writeConstraint 注入）——
寄存器模型必须保持 svd2rust 原生输出，unsafe 分层由 HAL 收敛。
（用户 2026-09-29 拍板：换源 PAC 后所有 demo 重新上板验收）
用法: python3 patch-svd.py <raw.svd> <patched.svd>
"""
import sys
import xml.etree.ElementTree as ET
import re

src, dst = sys.argv[1], sys.argv[2]
FIX_ACCESS = {"write": "write-only", "read": "read-only"}

tree = ET.parse(src)
root = tree.getroot()
n_acc = n_name = 0

for acc in root.iter("access"):
    if acc.text in FIX_ACCESS:
        acc.text = FIX_ACCESS[acc.text]
        n_acc += 1
for name in root.iter("name"):
    if name.text and (name.text != name.text.strip() or re.search(r"[\t\n\r]", name.text)):
        name.text = re.sub(r"[\t\n\r]", "", name.text).strip()
        n_name += 1

tree.write(dst, encoding="utf-8", xml_declaration=True)
print(f"patched: {n_acc} access, {n_name} names (minimal-runability-only) -> {dst}")
