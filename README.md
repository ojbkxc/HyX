# HyX

**极速 P2P 文件传输** — 基于 QUIC 的高性能跨平台传输内核，覆盖
Android / iOS / Windows / macOS / Linux，跨平台前端由 Flutter 驱动，
另附极简的原生 Android 轻量版。

Rust 负责传输内核（连接、发现、传输、断点续传、NAT 穿透），
Dart（Flutter）与 Kotlin（原生轻量版）共享同一套内核能力。

## 特性

- **QUIC 内核**：单条连续流、断点续传、TLS 1.3 端到端认证
- **单一路径引擎**：A/B 两个传输引擎融合为一条连续流，去逐块 fsync 与逐块确认
- **性能优化**：QUIC 高带宽窗口（96 MiB）、GSO 报文合并、收端后台写盘重叠网络读、
  压缩脱离 async 执行器（内嵌 `quinn-proto`，支持 Cubic/Brutal/BBR 拥塞控制）
- **数据完整性**：发送/接收双方独立增量 SHA-256，控制流上互相对比
- **局域网自动发现**：UDP 多播 beacon 广播 + 单播探测；
  单播探测带多轮重试、指数退避与失败冷却，广播间隔自适应节流
- **蓝牙跨子网发现**：开启蓝牙后，App 通过 BLE 广播本机局域网 IP 并扫描邻居 IP，
  补足**同一局域网内不同网段**设备的发现；在线状态仍由 Rust 单播探测判定
- **Wi-Fi Direct 直连**：两台设备都不连热点时，自动建组（MAC 小者当软热点），
  模拟小米互传原理实现无网络的点对点互传
- **NAT 穿透**：rendezvous 协调 + STUN 打洞，异地也能直连互传
- **配对互传**：扫码 / 配对码走 rendezvous 建立连接（需部署 rendezvous + STUN）
- **自动落盘**：收到的文件保存到系统「下载」目录
- **极致瘦身（轻量版）**：R8 代码压缩 + 资源缩减，原生 Android 版仅 arm64，
  安装包约 8–11 MB

> 说明：拥塞控制为 quinn 默认 Cubic（内嵌 `vendor/quinn-proto` 并支持 BBR /
> Brutal 开关）；本项目为单连接传输，未实现多链路（MPTCP/MPQUIC）聚合，
> 也未启用 TLS 0-RTT 会话恢复。

## 支持平台

| 平台 | 形态 | 说明 |
|------|------|------|
| Android | APK（arm64） | Flutter 前端 / 轻量版 Kotlin + Rust JNI |
| iOS | 未签名 IPA | 自行用免费 Apple ID 签名安装，见「iOS 安装」 |
| Windows | exe 安装包 / 便携 zip | x86_64 |
| macOS | zip（universal） | arm64 |
| Linux | tar.gz / deb / AppImage | x86_64 与 arm64 |
| CLI | 独立二进制 | Linux x86_64 / arm64、Windows x86_64 |

## 目录结构

```
HyX
├── core/             Rust 传输内核（连接、发现、传输、断点续传、NAT 穿透）
├── android/          原生 Android 轻量版（Kotlin + Compose + Material3 + Rust JNI）
├── app/              Flutter 跨平台前端（Windows / macOS / Linux / Android / iOS）
├── packages/
│   └── hyx_isolates/ Flutter Rust Bridge（FRB）：Rust 内核 → Dart 绑定 + cargokit 构建
├── mobile/           JNI 桥接（hyx-mobile.so，对接原生 Android）
├── cli/              命令行传输工具
├── rendezvous/       rendezvous 配对 / STUN 打洞协调
├── support/          打包与图标资源（deb / AppImage / Inno / 图标）
├── vendor/           quinn-proto 内嵌（含 Cubic / BBR / Brutal 拥塞控制）
└── tests/            集成与回环测试
```

核心能力都以 Rust 单代码库实现，因此所有平台行为一致；
Flutter 通过 `packages/hyx_isolates` 的 FRB 绑定调用内核，原生轻量版通过
`mobile` 的 JNI 桥调同一份内核。

## 构建

> 本项目不在本地强依赖环境，**编译与校验统一走 GitHub Actions**。

### CI 工作流

- [`ci.yml`](.github/workflows/ci.yml) — push main / PR 触发：
  Dart format、Flutter analyze & test、Rust fmt/clippy/test、
  Kotlin 编译验证（`compileDebugKotlin`）、版本一致性检查
- [`flutter.yml`](.github/workflows/flutter.yml) — push main / PR 触发：
  flutter_rust_bridge 生成 + Flutter analyze/test + FRB crate 编译/测试
- [`prerelease.yml`](.github/workflows/prerelease.yml) — push main 触发：
  自动构建 `v2.0.0` 预发布版（Flutter APK + 原生轻量 APK）
- [`release.yml`](.github/workflows/release.yml) — 推送 `v*` 标签或手动触发：
  生成全平台产物并发布正式版 GitHub Release

### 本地（可选）

本地 Rust：

```bash
cargo build --release --workspace
cargo clippy -p hyx-core --all-targets -- -D warnings
cargo test -p hyx-core
```

Android arm64（轻量版）：

```bash
cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build --release -p hyx-mobile --manifest-path mobile/Cargo.toml
cd android && gradle :app:assembleRelease
```

## 使用

1. 两台设备安装 HyX，开启蓝牙并连接到同一局域网（不同网段也可，靠蓝牙
   交换 IP + Rust 单播探测发现）
2. 发送端：选择文件，在设备页选择对端
3. 接收端：开启「开始接收」；在线状态由 Rust 单播探测实时判定
   （探到即在列表，探不到即离线）
4. **都不连热点时**：两台设备开启 Wi-Fi Direct 直连，自动建组互传
5. 异地互传：通过扫码 / 配对码走 rendezvous + NAT 穿透
6. 传输完成后文件出现在「下载」目录

## iOS 安装（免费自签）

iOS 版以**未签名 IPA** 形式发布（文件名 `HyX-<版本>-ios-unsigned.ipa`），
不出 App Store，无需付费 Apple Developer 账号。下载后用自己的 Apple ID
签名安装即可。

### 步骤

1. 从 [GitHub Release](.) 下载 `HyX-<版本>-ios-unsigned.ipa`。
2. 用以下任一工具签名安装到 iPhone/iPad：
   - **[Sideloadly](https://sideloadly.io)**（Windows/macOS，推荐）：用个人
     Apple ID 签名 IPA 并安装，需用数据线连接设备。
   - **[AltStore](https://altstore.io)**（Windows/macOS）：装好后在 AltStore
     里点「+」选 IPA 安装；AltStore 会**自动后台刷新**，避免 7 天过期。
3. 安装后到 **设置 → 通用 → VPN与设备管理**，找到你的开发者证书，点
   **信任**，即可打开 HyX。
4. 首次打开会弹「本地网络」权限，**必须允许**，否则搜不到附近设备；
   若启用蓝牙跨网段发现，还需授予蓝牙权限。

### 注意

- 个人（免费）Apple ID 签名的 App **每 7 天过期一次**，到期需重新签名
  安装。用 AltStore 并保持电脑常开可自动续签；Sideloadly 需手动重签。
- 每个免费 Apple ID 最多同时签 3 个 App，每周最多签 10 次。
- **TrollStore**：若设备系统在 iOS 14.0–16.6.1（部分 16.6.x）且已越狱/
   有 TrollStore，可用 TrollStore 永久签名 HyX，无需 7 天重签。详见
   [TrollStore 官方仓库](https://github.com/opa334/TrollStore)。

## 版本号

- **正式版**使用 `v1.0.x` 递增补丁号：手动推 `v*` 标签（如 `v1.0.1`）触发
  [release.yml](.github/workflows/release.yml)，或手动 dispatch 并填入版本号。
- **预发布版**固定为 `v2.0.0`，由 [prerelease.yml](.github/workflows/prerelease.yml)
  每次 push main 自动重建，不代表正式版本序列。

## License

MIT