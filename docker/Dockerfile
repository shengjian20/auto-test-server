# GD32F470 自动测试服务器构建容器
# 全程编译/烧录/调试在此容器内进行，USB 设备经透传访问
#
# 构建镜像:  docker build -t auto-test-server:latest docker/
# 运行容器:  ./docker/run.sh
#
# 网络策略：不走代理，全量使用国内源（apt=USTC, rustup/cargo=rsproxy, pip=清华, GitHub=ghfast）
# 基础镜像为 daocloud 源 pull 的 ubuntu:24.04（规避 dockerd 直连 Docker Hub 的 DNS 污染）
FROM docker.m.daocloud.io/library/ubuntu:24.04

ARG RUST_VERSION=stable
ARG TARGET=thumbv7em-none-eabihf

ENV DEBIAN_FRONTEND=noninteractive
ENV LANG=C.UTF-8 LC_ALL=C.UTF-8

# apt 切换 USTC 国内源：裸镜像无 ca-certificates，HTTPS 握手必挂，故 apt 阶段用 HTTP，
# ca-certificates 装好后的后续步骤（rustup/curl）用 HTTPS
RUN sed -i 's|http://archive.ubuntu.com/ubuntu|http://mirrors.ustc.edu.cn/ubuntu|g; s|http://security.ubuntu.com/ubuntu|http://mirrors.ustc.edu.cn/ubuntu|g' /etc/apt/sources.list.d/ubuntu.sources /etc/apt/sources.list 2>/dev/null; \
    grep -q "mirrors.ustc.edu.cn" /etc/apt/sources.list.d/ubuntu.sources /etc/apt/sources.list 2>/dev/null || exit 1

# 编译工具链 + USB(udev) + 调试依赖 + gosu(entrypoint 需要)
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config autoconf automake libtool \
        git curl ca-certificates file xz-utils \
        libudev-dev libusb-1.0-0-dev \
        python3 python3-pip \
        openssh-client \
        udev \
        gosu \
    && rm -rf /var/lib/apt/lists/*

# Rust 工具链：rustup/cargo 全走 rsproxy.cn（字节跳动 Rust 镜像）
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH \
    RUSTUP_DIST_SERVER=https://rsproxy.cn \
    RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
COPY cargo-config.toml /usr/local/cargo/config.toml
RUN curl -sSf https://rsproxy.cn/rustup-init.sh | sh -s -- -y \
        --default-toolchain ${RUST_VERSION} \
        --profile minimal \
    && rustup target add ${TARGET} \
    && rustup component add rustfmt clippy llvm-tools-preview \
    && cargo install cargo-binutils \
    && rm -rf /usr/local/cargo/registry/src

# probe-rs：官方二进制 vendor 进仓库（docker/vendor/，经 gh-proxy 加速预下载），
# 构建零外网依赖、版本固定；更新步骤见 docker/vendor/README.md
COPY vendor/probe-rs-tools-0.32.0-x86_64.tar.xz /tmp/probe-rs.tar.xz
RUN tar -xf /tmp/probe-rs.tar.xz -C /tmp \
    && find /tmp -maxdepth 3 -type f -name 'probe-rs' -exec mv {} /usr/local/bin/ \; \
    && chmod +x /usr/local/bin/probe-rs \
    && rm -rf /tmp/probe-rs*

# pip 全局切换清华源
RUN printf '[global]\nindex-url = https://pypi.tuna.tsinghua.edu.cn/simple\n' > /etc/pip.conf

WORKDIR /workspace
VOLUME ["/workspace"]

# 非 root（与宿主机 UID/GID 一致由 run.sh 传入 USER_UID/USER_GID）
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["bash"]
