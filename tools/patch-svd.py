#!/usr/bin/env python3
"""DFP SVD 清洗：修复 svd2rust 无法解析的垃圾。
1. 非法 access 枚举（write/read -> write-only/read-only），DFP 3.5.0 有 5 处
2. name 元素中的控制字符/空白（如 RTC_T\tamper），剥离后保证为合法 Ident 片段"""
import sys
import xml.etree.ElementTree as ET
import re

src, dst = sys.argv[1], sys.argv[2]
FIX_ACCESS = {"write": "write-only", "read": "read-only"}

tree = ET.parse(src)
n_acc = n_name = 0
for acc in tree.getroot().iter("access"):
    if acc.text in FIX_ACCESS:
        acc.text = FIX_ACCESS[acc.text]
        n_acc += 1
for name in tree.getroot().iter("name"):
    if name.text and (name.text != name.text.strip() or re.search(r"[\t\n\r]", name.text)):
        name.text = re.sub(r"[\t\n\r]", "", name.text).strip()
        n_name += 1
tree.write(dst, encoding="utf-8", xml_declaration=True)
print(f"patched: {n_acc} access, {n_name} names -> {dst}")
