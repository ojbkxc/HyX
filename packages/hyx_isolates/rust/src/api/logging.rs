//! 日志回调 API。
//!
//! 对应 `mobile/src/lib.rs` 的 `hyxSetLogCallback`：注册全局 `tracing` 订阅者，
//! 将 Rust 侧所有 `tracing::event!` 转发到上层。
//!
//! 与 mobile 差异：
//! - mobile 通过 JNI `GlobalRef` 持有 Kotlin `LogCallback`，每事件 `attach_current_thread`
//!   后 `call_method("onLog", "(ILjava/lang/String;Ljava/lang/String;)V", ...)`；
//! - FRB 版本用 `StreamSink<RsLogEvent>`，Rust 直接 `sink.add(RsLogEvent { ... })`，
//!   Dart 侧 `Stream.listen`。无需跨线程 `JNIEnv` 管理。
//! - mobile 用自定义 `JniLogLayer`；FRB 版本用 `StreamSinkLogLayer`，结构相同但
//!   输出到 `StreamSink` 而非 JNI。

use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;
use flutter_rust_bridge::frb;
use tracing_core::{Event, Level, Subscriber};
use tracing_subscriber::field::Visit;
use tracing_subscriber::layer::{Context, Layer};

use crate::api::model::{RsLogEvent, RsLogLevel};
use crate::frb_generated::StreamSink;

/// 全局日志 `StreamSink`。`OnceLock` 保证首次注册后回调不变（与 mobile `LOG_CB` 语义一致）。
static LOG_SINK: OnceLock<Arc<Mutex<Option<StreamSink<RsLogEvent>>>>> = OnceLock::new();

fn log_sink() -> &'static Arc<Mutex<Option<StreamSink<RsLogEvent>>>> {
    LOG_SINK.get_or_init(|| Arc::new(Mutex::new(None)))
}

/// `tracing-subscriber` Layer：将每个 event 转发到 `StreamSink<RsLogEvent>`。
///
/// 对应 mobile `JniLogLayer`，但用 `StreamSink` 替代 JNI `call_method`。
struct StreamSinkLogLayer;

impl<S: Subscriber> Layer<S> for StreamSinkLogLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let sink_guard = log_sink();
        let sink_cell = sink_guard.lock().unwrap_or_else(|p| p.into_inner());
        let Some(sink) = sink_cell.as_ref() else {
            return;
        };

        let level = level_to_enum(event.metadata().level());
        let tag = event.metadata().target().to_string();

        let mut visitor = FieldCollector::default();
        event.record(&mut visitor);
        let message = visitor.finish();

        let _ = sink.add(RsLogEvent {
            level,
            tag,
            message,
        });
    }
}

/// `tracing` Level → `RsLogLevel`。对应 mobile `level_to_int`。
fn level_to_enum(l: &Level) -> RsLogLevel {
    match *l {
        Level::TRACE => RsLogLevel::Trace,
        Level::DEBUG => RsLogLevel::Debug,
        Level::INFO => RsLogLevel::Info,
        Level::WARN => RsLogLevel::Warn,
        Level::ERROR => RsLogLevel::Error,
    }
}

/// `tracing` 字段访问者：收集 `message` 字段 + 其余字段为 `key=value`。
/// 对应 mobile `FieldCollector`，逐字复制以保持日志格式一致。
#[derive(Default)]
struct FieldCollector {
    message: String,
    extras: Vec<String>,
}

impl Visit for FieldCollector {
    fn record_debug(&mut self, field: &tracing_core::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        } else {
            self.extras.push(format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing_core::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.extras.push(format!("{}={}", field.name(), value));
        }
    }
}

impl FieldCollector {
    fn finish(mut self) -> String {
        if self.extras.is_empty() {
            self.message
        } else if self.message.is_empty() {
            self.extras.join(" ")
        } else {
            self.extras.insert(0, self.message);
            self.extras.join(" ")
        }
    }
}

/// 注册全局日志回调并安装 `tracing` 订阅者。
///
/// 对应 mobile `Java_com_ojbkxc_hyx_core_HyXNative_hyxSetLogCallback`：
/// - 存储 `sink` 到全局 `OnceLock`（首次注册后后续调用保留首次的 sink，与 mobile 一致）；
/// - 安装 `StreamSinkLogLayer` 作为全局 `tracing` 订阅者；
/// - 桥接 `log` crate 到 `tracing`，使 hyx-core 内的 `log::info!` 等也转发到 sink。
///
/// 调用时机：Dart 侧 `RustLib.init()` 之后、任何可能产生 `tracing` 事件的调用之前。
///
/// # Errors
///
/// 安装订阅者失败（已存在全局订阅者）时返回错误，但 sink 仍被注册——
/// 与 mobile "first call wins" 语义一致。
#[frb]
pub fn set_log_callback(sink: StreamSink<RsLogEvent>) -> Result<()> {
    {
        let cell = log_sink().lock().unwrap_or_else(|p| p.into_inner());
        if cell.is_none() {
            drop(cell);
            let mut cell = log_sink().lock().unwrap_or_else(|p| p.into_inner());
            *cell = Some(sink);
        }
    }

    use tracing_subscriber::prelude::*;
    let subscriber = tracing_subscriber::registry().with(StreamSinkLogLayer);
    let _ = tracing::subscriber::set_global_default(subscriber);

    // 桥接 `log` crate → `tracing`，与 mobile 一致。
    let _ = tracing_log::LogTracer::init();

    Ok(())
}

/// 启用 debug 级日志（开发模式）。
///
/// 对应 LocalSend `logging::enable_debug_logging`：在 Android 上路由到 logcat，
/// 其他平台用 `fmt` 订阅者。本函数独立于 `set_log_callback`，用于无 Dart 监听时
/// 的本地调试。
#[frb]
pub fn enable_debug_logging() -> Result<()> {
    #[cfg(target_os = "android")]
    {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        tracing_subscriber::registry()
            .with(tracing_subscriber::filter::LevelFilter::DEBUG)
            .with(tracing_android::layer("hyx_rust")?)
            .try_init()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    }

    #[cfg(not(target_os = "android"))]
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    Ok(())
}
