package com.ojbkxc.hyx.core

import android.os.Build
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/**
 * 更新信息。
 *
 * @param version     新版本号（语义化版本 x.y.z）
 * @param downloadUrl 下载链接（自定义下载站直链或 GitHub Release html_url）
 * @param releaseNotes 发行说明，可能为空
 */
data class UpdateInfo(
    val version: String,
    val downloadUrl: String,
    val releaseNotes: String?
)

/**
 * 多平台更新检测器（对齐 Flutter app/lib/util/update_checker.dart）。
 *
 * 检测顺序：
 * 1. 自定义下载站 https://downloads.lxseek.com/HyX/latest.json（优先）
 * 2. GitHub Releases（回退）
 *
 * 使用 java.net.HttpURLConnection，无需引入 OkHttp 等第三方依赖。
 * 网络请求在 [Dispatchers.IO] 上执行，调用方只需在协程中调用 [check]。
 */
object UpdateChecker {
    private const val TAG = "UpdateChecker"
    private const val CUSTOM_BASE_URL = "https://downloads.lxseek.com/HyX"


    /**
     * 检测更新，返回 [UpdateInfo] 或 null。
     *
     * @param currentVersion 当前版本号（语义化版本 x.y.z）
     *
     * 参照 Lxchat UpdateChecker 的检测流程：
     * 1. 优先检测自定义下载站 — 检测成功后（无论是否有新版本）直接返回，不再回退 GitHub
     * 2. 自定义下载站不可达时才回退到 GitHub Releases（过滤掉 prerelease）
     */
    suspend fun check(currentVersion: String): UpdateInfo? = withContext(Dispatchers.IO) {
        // 1. 优先检测自定义下载站
        try {
            val result = checkCustomDownloadSite(currentVersion)
            result.fold(
                onSuccess = { info ->
                    // 检测成功：info 非空表示有新版本，null 表示已是最新
                    if (info != null) {
                        HyXLog.i(TAG, "自定义下载站发现新版本: ${info.version}")
                    }
                    return@withContext info
                },
                onFailure = { e ->
                    // 检测失败（网络错误等），回退到 GitHub
                    HyXLog.w(TAG, "自定义下载站检测失败: ${e.message}")
                }
            )
        } catch (t: Throwable) {
            HyXLog.w(TAG, "自定义下载站检测异常: ${t.message}")
        }

        // 2. 回退到 GitHub Releases（过滤 prerelease）
        try {
            val info = checkGitHubReleases(currentVersion)
            if (info != null) {
                HyXLog.i(TAG, "GitHub Releases 发现新版本: ${info.version}")
            }
            return@withContext info
        } catch (t: Throwable) {
            HyXLog.w(TAG, "GitHub Releases 检测失败: ${t.message}")
        }

        null
    }

    /**
     * 检测自定义下载站。
     *
     * 返回三种结果：
     * - `Result.success(null)`：检测成功，但当前版本已是最新，无需更新
     * - `Result.success(info)`：检测成功，发现新版本
     * - `Result.failure`：检测失败（网络错误等），调用方应回退到 GitHub
     *
     * 兼容两种 JSON 格式：
     * - 简化格式：{"version", "downloadUrl", "releaseNotes"}
     * - Flutter 同源格式：{"version", "files": {platformKey: fileName}, "body"}
     *
     * 参照 Lxchat UpdateChecker：检测成功后（无论是否有新版本）都直接返回，
     * 不再回退到 GitHub —— 避免自定义下载站说"已是最新"但 GitHub 的
     * prerelease 又弹窗提示更新的问题。
     */
    private fun checkCustomDownloadSite(currentVersion: String): Result<UpdateInfo?> {
        val raw = httpGet("$CUSTOM_BASE_URL/latest.json")
            ?: return Result.failure(Exception("自定义下载站不可达"))
        val data = JSONObject(raw)
        val version = data.optString("version")
        if (version.isEmpty()) return Result.failure(Exception("version 字段为空"))

        // 版本不比当前新 → 检测成功，无需更新
        if (compareVersions(version, currentVersion) <= 0) {
            return Result.success(null)
        }

        // 解析下载链接：优先 downloadUrl，回退到 files[platformKey]
        val directUrl = data.optString("downloadUrl", "")
        val resolvedUrl = if (directUrl.isNotEmpty()) {
            directUrl
        } else {
            val files = data.optJSONObject("files")
            val platformKey = getPlatformKey()
            val fileName = files?.optString(platformKey, "") ?: ""
            if (fileName.isNotEmpty()) "$CUSTOM_BASE_URL/$fileName" else ""
        }
        if (resolvedUrl.isEmpty()) return Result.failure(Exception("无法解析下载链接"))

        // 解析发行说明：优先 releaseNotes，回退到 body
        val releaseNotes = when {
            data.has("releaseNotes") -> data.optString("releaseNotes")
            data.has("body") -> data.optString("body")
            else -> ""
        }

        return Result.success(
            UpdateInfo(
                version = version,
                downloadUrl = resolvedUrl,
                releaseNotes = releaseNotes.ifEmpty { null }
            )
        )
    }

    /**
     * 检测 GitHub Releases。
     *
     * 参照 Lxchat UpdateChecker：用 `releases?per_page=10` 列出多个 release，
     * 过滤掉 prerelease，只取最新的正式 release。`releases/latest` 端点虽然
     * 也不返回 prerelease，但在某些边缘情况下（如所有 release 都是 prerelease）
     * 会返回 404，用列表 + 过滤更健壮。
     */
    private fun checkGitHubReleases(currentVersion: String): UpdateInfo? {
        val raw = httpGet("https://api.github.com/repos/ojbkxc/HyX/releases?per_page=10") ?: return null
        val releases = JSONArray(raw)
        // 找第一个非 prerelease、非 draft 的 release
        var stableRelease: JSONObject? = null
        for (i in 0 until releases.length()) {
            val rel = releases.optJSONObject(i) ?: continue
            if (rel.optBoolean("prerelease", false)) continue
            if (rel.optBoolean("draft", false)) continue
            stableRelease = rel
            break
        }
        val release = stableRelease ?: return null
        val tagName = release.optString("tag_name")
        if (tagName.isEmpty()) return null
        val version = tagName.removePrefix("v")
        if (compareVersions(version, currentVersion) <= 0) return null
        val htmlUrl = release.optString("html_url")
        if (htmlUrl.isEmpty()) return null
        val releaseNotes = release.optString("body", "")
        return UpdateInfo(
            version = version,
            downloadUrl = htmlUrl,
            releaseNotes = releaseNotes.ifEmpty { null }
        )
    }

    /**
     * 获取当前平台的 key（对应 latest.json files 中的 key）。
     *
     * 对齐 Flutter 侧 _getPlatformKey() 的 Android 分支。
     */
    private fun getPlatformKey(): String {
        val abi = Build.SUPPORTED_ABIS.firstOrNull() ?: ""
        return when {
            abi.contains("arm64") -> "android-arm64v8"
            abi.contains("x86_64") -> "android-x64"
            else -> "android-arm32v7"
        }
    }

    /**
     * 比较语义化版本号，返回正数表示 a > b，负数表示 a < b，0 表示相等。
     *
     * 对齐 Flutter 侧 _compareVersions()。
     */
    private fun compareVersions(a: String, b: String): Int {
        val partsA = a.split(".").map { it.toIntOrNull() ?: 0 }
        val partsB = b.split(".").map { it.toIntOrNull() ?: 0 }
        val maxLen = maxOf(partsA.size, partsB.size)
        for (i in 0 until maxLen) {
            val va = partsA.getOrElse(i) { 0 }
            val vb = partsB.getOrElse(i) { 0 }
            if (va != vb) return va - vb
        }
        return 0
    }

    /**
     * 简单的 HTTP GET，返回响应体字符串或 null。
     *
     * - 连接/读取超时各 10s/15s
     * - GitHub API 要求 User-Agent，否则返回 403
     * - 仅接受 HTTP 200
     */
    private fun httpGet(urlStr: String): String? {
        val conn = (URL(urlStr).openConnection() as HttpURLConnection).apply {
            requestMethod = "GET"
            connectTimeout = 10_000
            readTimeout = 15_000
            setRequestProperty("User-Agent", "HyX-Android-UpdateChecker")
            setRequestProperty("Accept", "application/json")
        }
        try {
            if (conn.responseCode != HttpURLConnection.HTTP_OK) return null
            return conn.inputStream.bufferedReader().use { it.readText() }
        } finally {
            conn.disconnect()
        }
    }
}