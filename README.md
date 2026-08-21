# HyX

极速 P2P 文件传输。基于 QUIC，Rust 实现高速传输内核，Android 原生界面。

## 特性

- **QUIC 内核**：单条连续流、断点续传、TLS 1.3 端到端认证
- **单一路径引擎**：A/B 两个传输引擎融合为一条连续流，去逐块 fsync 与逐块确认
- **性能优化**：QUIC 高带宽窗口（96 MiB）、GSO 报文合并、收端后台写盘重叠网络读、压缩脱离 async 执行器
- **数据完整性**：发送/接收双方独立增量 SHA-256，控制流上互相对比
- **局域网自动发现**：UDP beacon 扫描附近设备
- **配对互传**：扫码 / 配对码走 rendezvous 建立连接（需部署 rendezvous + STUN 服务器）
- **自动落盘**：收到的文件保存到系统「下载」目录
- **极致瘦身**：R8 代码压缩 + 资源缩减，仅 arm64，安装包约 8–11 MB

> 说明：拥塞控制为 quinn 默认 Cubic（quinn 无公开 BBR 开关）；本项目为单连接
> 传输，未实现多链路（MPTCP/MPQUIC）聚合，也未启用 TLS 0-RTT 会话恢复。

## 目录结构

```
HyX
├── core/          Rust 传输内核（连接、发现、传输、断点续传）
├── mobile/        JNI 桥接（hyx-mobile.so，对接 Android）
├── cli/           命令行传输工具
├── rendezvous/    rendezvous 配对协调
└── android/       Android 应用（Kotlin + Compose + Material3）
```

## 构建

### Android arm64 APK

本地需要：Android SDK（platform 35 / build-tools 35）、NDK 27、Rust + `cargo-ndk`。

```bash
# 编译 Rust 内核为 arm64 动态库
cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build --release \
  -p hyx-mobile --manifest-path mobile/Cargo.toml

# 打包 release APK（R8 精简）
cd android
gradle :app:assembleRelease
```

CI 已配置，提交即自动构建：

- `ci.yml` — push/PR 触发，Rust 严检（fmt/clippy/test）+ 构建 arm64 release APK
- `release.yml` — 推送 `v*` 标签或手动触发，写入版本号并发布 GitHub Release

### Rust（core / cli）

```bash
cargo build --release --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## 使用

1. 两台手机安装 HyX，连接同一 Wi-Fi
2. 发送端：传输页选择文件，设备页选择对端
3. 接收端：传输页点「开始接收」（或通过配对码/扫码建立连接）
4. 传输完成后文件自动出现在「下载」目录

## 版本号

版本从 Git 标签或手动输入解析，`versionCode` 由 semver 计算，写入
`android/app/build.gradle.kts`。发布流程参照 TaskMod。

## License

MIT