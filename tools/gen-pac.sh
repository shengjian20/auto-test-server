#!/bin/bash
# 从 DFP SVD 生成 gd32f470 PAC（在构建容器内运行）
# 用法: ./tools/gen-pac.sh
# 流程: 原始 DFP SVD -> patch-svd.py 清洗 -> svd2rust 0.37（单 lib.rs，form 拆分对 0.37 结构无效已省略）
set -euo pipefail
cd "$(dirname "$0")/.."

SVD_RAW=doc/svd/GD32F4xx_DFP3.5.0.svd
SVD=/tmp/gd32f470_patched.svd
OUT=src/pac/gd32f470
GEN=/tmp/pac-gen

python3 tools/patch-svd.py "$SVD_RAW" "$SVD"
mkdir -p "$GEN" "$OUT/src"
svd2rust --target cortex-m -i "$SVD" -o "$GEN"
cp "$GEN/lib.rs" "$OUT/src/lib.rs"
cp "$GEN/build.rs" "$OUT/build.rs"
cp "$GEN/device.x" "$OUT/device.x"

echo "PAC generated into $OUT"
