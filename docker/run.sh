#!/bin/bash
# 启动构建容器（宿主机执行）
# 用法: ./docker/run.sh [命令...]
# 默认进入交互 shell；USB 全透传（probe-rs 烧录需要）
set -e
cd "$(dirname "$0")/.."

exec docker run -it --rm \
    --privileged \
    -v /dev/bus/usb:/dev/bus/usb \
    -v "$(pwd):/workspace" \
    -e USER_UID="$(id -u)" -e USER_GID="$(id -g)" \
    auto-test-server:latest "$@"
