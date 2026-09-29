# docker/vendor/

预下载的二进制，镜像构建零外网依赖。

## probe-rs-tools-0.32.0-x86_64.tar.xz

来源：`https://github.com/probe-rs/probe-rs/releases/download/v0.32.0/probe-rs-tools-x86_64-unknown-linux-gnu.tar.xz`
（经 `https://gh-proxy.com/` 前缀加速下载）

sha256: `c2ccc46049e52a5d403ef212078cd637ecda55b662708327960558f83e851ff5`

更新方法：

```bash
VER=0.33.0  # 目标版本
curl -L "https://gh-proxy.com/https://github.com/probe-rs/probe-rs/releases/download/v${VER}/probe-rs-tools-x86_64-unknown-linux-gnu.tar.xz" \
    -o vendor/probe-rs-tools-${VER}-x86_64.tar.xz
tar -tf vendor/probe-rs-tools-${VER}-x86_64.tar.xz   # 校验 tar 完整
# 同步修改 Dockerfile 中的 COPY 文件名与 PROBE_RS 版本引用
```

下载不稳定时的备选前缀：`https://mirror.ghproxy.com/`、`https://ghproxy.net/`，
支持断点续传 `-C -` 多次重试。
