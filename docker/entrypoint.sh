#!/bin/bash
# 容器入口：保证以与宿主机挂载目录权限一致的 UID/GID 运行构建命令
set -e

USER_UID=${USER_UID:-1000}
USER_GID=${USER_GID:-1000}

# 基础镜像自带 ubuntu 用户(1000)：UID 命中则复用；否则建同名用户
if getent passwd "$USER_UID" >/dev/null; then
    RUN_USER=$(getent passwd "$USER_UID" | cut -d: -f1)
    [ "$(id -g "$RUN_USER")" = "$USER_GID" ] || groupmod -g "$USER_GID" "$RUN_USER"
else
    RUN_USER=builder
    getent group "$USER_GID" >/dev/null || groupadd -g "$USER_GID" "$RUN_USER"
    useradd -m -u "$USER_UID" -g "$USER_GID" -s /bin/bash "$RUN_USER"
fi

export CARGO_HOME=/usr/local/cargo
export RUSTUP_HOME=/usr/local/rustup
export PATH=/usr/local/cargo/bin:$PATH

exec gosu "$RUN_USER" "$@"
