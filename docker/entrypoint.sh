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

# rustup 工具链只读共享(/usr/local/rustup)；cargo 注册表/缓存按用户可写(home)
RUN_HOME=$(getent passwd "$RUN_USER" | cut -d: -f6)
export RUSTUP_HOME=/usr/local/rustup
export CARGO_HOME="$RUN_HOME/.cargo"
export PATH=/usr/local/cargo/bin:"$CARGO_HOME"/bin:$PATH

# 用户级 cargo 沿用镜像内的国内源配置；基础镜像的 /home/ubuntu 属 root，需修正属主
mkdir -p "$CARGO_HOME"
cp /usr/local/cargo/config.toml "$CARGO_HOME/config.toml" 2>/dev/null || true
chown -R "$USER_UID:$USER_GID" "$RUN_HOME" 2>/dev/null || true

exec gosu "$RUN_USER" "$@"
