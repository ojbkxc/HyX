# 剩余修复任务

## 环境说明（重要）
- 项目路径: D:/GitHub/HyX
- 当前机器没有 flutter SDK，flutter_rust_bridge_codegen 未安装。
- FRB 生成文件被 gitignore（packages/hyx_isolates/rust/src/frb_generated.rs 和 packages/hyx_isolates/lib/rust/ 不存在）。
- 因此 cargo check 会因 frb_generated.rs 缺失报错 E0583，这是环境问题，不是代码错误，忽略即可。
- 不要尝试安装 flutter SDK 或 FRB codegen（耗时长且可能失败）。只做代码修改。

## 任务1: Dart 侧同步 fileName/peerAddress（完成根因1）

文件: app/lib/provider/transfer_provider.dart
位置: _UpdateProgressAction 的 copyWith（第407-417行）

当前代码:
```dart
return state.copyWith(
  direction: dir,
  status: event.status,
  transferred: event.transferred.toInt(),
  total: event.total.toInt(),
  speed: event.speed,
  startTime: shouldSetStart ? DateTime.now() : null,
  endTime: isDone ? DateTime.now() : null,
  errorMessage: event.message,
  peerFingerprint: event.peerFingerprint,
);
```

改为（加两行）:
```dart
return state.copyWith(
  direction: dir,
  status: event.status,
  transferred: event.transferred.toInt(),
  total: event.total.toInt(),
  speed: event.speed,
  startTime: shouldSetStart ? DateTime.now() : null,
  endTime: isDone ? DateTime.now() : null,
  errorMessage: event.message,
  peerFingerprint: event.peerFingerprint,
  fileName: event.fileName ?? state.fileName,
  peerAddress: event.peerAddress ?? state.peerAddress,
);
```

说明: event.fileName/peerAddress 是 RsProgressEvent 新加的 Option<String> 字段，FRB codegen 后会生成 Dart 的 String? 字段。用 ?? 合并：event 非 null 时覆盖（接收方回填），null 时保留原 state 值（发送方启动时已设）。

## 任务2: receive_to 加重连逻辑（根因2）

文件: packages/hyx_isolates/rust/src/api/transfer.rs（或 session.rs，先搜索 receive_to 定义位置）

背景: send_path 有重连逻辑（搜索 send_path 函数，看它的 loop 和 ReconnectConfig/is_recoverable 用法），遇到可恢复错误（Network/Timeout/Disconnected/Quic/HolePunchFailed）会重连继续，最多 5 次。但 receive_to 没有重连逻辑，任何错误直接 Failed。这是 receive failed 但 send completed 的根因之一。

要求:
1. 先读 send_path 的重连实现，理解 ReconnectConfig、is_recoverable()、重连 loop 结构。
2. 在 receive_to 里加类似的重连 loop：遇到 is_recoverable() 错误时，重试（重新 accept + receive_into），最多重试次数对齐 send_path（如 5 次）。
3. 注意接收方重连需要重新 accept（等待新连接），与发送方重连（重新 connect）不同。重连后要重新 emit peer_address 事件。
4. 保持代码风格一致。不可恢复的错误直接 Failed（原行为）。
5. 如果 receive_to 结构不适合加重连（比如 accept 在外层 start_listener），则在合适的层级加重连逻辑。先读代码理解结构再改。

## 验证
- Rust: 运行 cargo check --manifest-path packages/hyx_isolates/rust/Cargo.toml，忽略 frb_generated.rs 缺失错误（E0583），只看是否有我们修改导致的编译错误。
- Dart: 无法验证编译（缺 FRB 生成文件），只确保代码修改正确。

## 返回
完成后报告: 改了哪些文件（路径+行号）、改了什么、cargo check 结果（忽略 E0583）、遇到的问题。