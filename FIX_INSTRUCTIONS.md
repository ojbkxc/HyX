# 修复任务：接收方历史记录 fileName/peerAddress 永远为空

## 项目
路径: D:/GitHub/HyX (Flutter + Rust 混合，flutter_rust_bridge)

## Bug 根因(已确认)
1. RsProgressEvent 结构体 (packages/hyx_isolates/rust/src/api/model.rs 第69-93行) 没有 file_name 和 peer_address 字段。
2. Dart 接收流程 (app/lib/provider/transfer_provider.dart): StartReceiveAction/StartAutoListenAction 不设置 fileName/peerAddress; _UpdateProgressAction (第364-419行) copyWith (第407-417行) 没从 event 同步这两个字段。
3. 历史记录写入 (app/lib/pages/transfer_progress_sheet.dart 第130-139行) 直接取 st.fileName/st.peerAddress，所以接收记录必然为空。
4. 发送流程正常: StartSendAction (transfer_provider.dart 第308-309行) 启动时就设置了 fileName/peerAddress。

## 修复步骤

### 步骤1: Rust model.rs
在 packages/hyx_isolates/rust/src/api/model.rs 的 RsProgressEvent 结构体 (第69-93行) 末尾(peer_fingerprint 之后)加两个字段:
```rust
/// 文件名(接收方在收到 TransferInfo 后回填；发送方在启动时已知)。
pub file_name: Option<String>,
/// 对端地址(接收方在 accept 拿到连接后回填；发送方在启动时已知)。
pub peer_address: Option<String>,
```

### 步骤2: Rust transfer.rs
packages/hyx_isolates/rust/src/api/transfer.rs 中所有构造 RsProgressEvent 的地方(约8处: 第150,193,277,719,818行等)都要补上 file_name/peer_address 字段。
- 发送流程(connect 相关): file_name 填文件名(从 path 提取), peer_address 填 peerAddress 参数。
- 接收流程(start_listener/receive_into 相关): 初始为 None；在 accept 成功拿到连接后，把连接的 remote address 填入 peer_address；在收到 TransferInfo 后把文件名填入 file_name。
- emit_peer_fingerprint_cached (第149行) 等辅助函数: file_name=None, peer_address=None。
- progress_sink (第166行附近) 产生的 Transferring 事件: 需要把当前 file_name/peer_address 透传(可能需要给 progress_sink 加参数或用闭包捕获)。

注意: 需要读 transfer.rs 和 session.rs 理解接收流程，找到 accept 后获取 remote address 的位置，以及 receive_folder 收到 TransferInfo 的位置。TransferInfo 里应有文件名信息。

### 步骤3: 重新生成 FRB 绑定
在项目根目录或 packages/hyx_isolates 下执行 flutter_rust_bridge 代码生成命令(查看 pubspec.yaml / Makefile / justfile / .github/workflows 找 codegen 命令，常见: `flutter_rust_bridge_codegen generate` 或 `cargo run -p flutter_rust_bridge_codegen -- generate`)。生成后确认 packages/hyx_isolates/rust/api/model.dart 里 RsProgressEvent 有 fileName/peerAddress 字段。

### 步骤4: Dart transfer_provider.dart
在 app/lib/provider/transfer_provider.dart 的 _UpdateProgressAction (第407-417行) copyWith 里加:
```dart
fileName: event.fileName ?? state.fileName,
peerAddress: event.peerAddress ?? state.peerAddress,
```
(仅当 event 字段非 null 时覆盖，避免发送流程已设的值被空覆盖)

### 步骤5: 构建验证
- Rust: 在 packages/hyx_isolates/rust 下 `cargo check` 确认编译通过。
- FRB: 确认生成的 Dart 镜像有新字段。
- Dart: 在 app/ 下 `flutter analyze` 确认无错误。
- 如能完整构建更好: `flutter build apk --debug` (可能耗时，可选)。

## 约束
- 不要改变发送流程的现有行为(发送方 fileName/peerAddress 已正确设置)。
- 接收方 event 里 file_name/peer_address 用 Option，None 时 Dart 侧保留原 state 值。
- 保持 Rust 代码风格(看现有构造点的写法)。
- 修改前先用 Read 读取每个要改的文件确认当前内容。

## 返回
完成后报告: 改了哪些文件(路径+行号)、FRB 生成是否成功、cargo check / flutter analyze 结果、遇到的问题。