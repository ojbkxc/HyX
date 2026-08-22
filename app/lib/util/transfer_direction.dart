/// 传输方向。对应 Rust `RsTransferDirection`，但仅在 Dart 端使用。
///
/// Rust 侧定义了此枚举但未在任何 API 函数签名中使用，
/// 因此 FRB 不会生成 Dart 镜像。在此手动定义以供状态管理使用。
enum RsTransferDirection {
  send,
  receive,
}