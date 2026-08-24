package com.ojbkxc.hyx_app

import android.content.Intent
import android.os.Environment
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

private const val CHANNEL = "com.ojbkxc.hyx_app/hyx"

class MainActivity : FlutterActivity() {
    /// share_handler 会在 Dart 侧尚未订阅 sharedMediaStream 时丢弃通过
    /// onNewIntent 到达的分享 intent，这在 singleTask 模式下应用仍在启动、
    /// 已有 task 被重新拉起时尤其常见。把这些 intent 暂存起来，等 Dart 侧
    /// 报告就绪（"shareIntentReady"）后再按常规插件路径重放。
    private val pendingShareIntents = mutableListOf<Intent>()
    private var shareIntentReady = false

    override fun onNewIntent(intent: Intent) {
        if (!shareIntentReady && (intent.action == Intent.ACTION_SEND || intent.action == Intent.ACTION_SEND_MULTIPLE)) {
            pendingShareIntents.add(intent)
            return
        }
        super.onNewIntent(intent)
    }

    private fun onShareIntentReady() {
        shareIntentReady = true
        val pending = pendingShareIntents.toList()
        pendingShareIntents.clear()
        for (intent in pending) {
            super.onNewIntent(intent)
        }
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(
            flutterEngine.dartExecutor.binaryMessenger,
            CHANNEL
        ).setMethodCallHandler { call, result ->
            when (call.method) {
                "getDownloadsDirectory" -> {
                    result.success(getDownloadsDirectory())
                }
                "shareIntentReady" -> {
                    onShareIntentReady()
                    result.success(null)
                }
                else -> result.notImplemented()
            }
        }
    }

    /// Absolute path of the shared "Download" directory (usually /storage/emulated/0/Download).
    @Suppress("DEPRECATION")
    private fun getDownloadsDirectory(): String {
        return Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS).absolutePath
    }
}
