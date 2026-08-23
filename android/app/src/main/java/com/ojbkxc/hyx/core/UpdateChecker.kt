package com.ojbkxc.hyx.core

import android.os.Build
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
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
    private const val GITHUB_API =
        "https://api.github.com/repos/ojbkxc/HyX/releases/latest"

    /**
     * 检测更新，返回 [UpdateInfo] 或 null。
     *
     * @param currentVersion 当前版本号（语义化版本 x.y.z）
     */
    suspend fun check(currentVersion: String): UpdateInfo? = withContext(Dispatchers.IO) {
        // 1. 优先检测自定义下载站
        try {
            val info = checkCustomDownloadSite(currentVersion)
            if (info != null) {
                HyXLog.i(TAG, "自定义下载站发现新版本: ${info.version}")
                return@withContext info
            }
        } catch (t: Throwable) {
            // 自定义下载站检测失败，回退到 GitHub
            HyXLog.w(TAG, "自定义下载站检测失败: ${t.message}")
        }

        // 2. 回退到 GitHub Releases
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
     * 兼容两种 JSON 格式：
     * - 简化格式：{"version", "downloadUrl", "releaseNotes"}
     * - Flutter 同源格式：{"version", "files": {platformKey: fileName}, "body"}
     */
    private fun checkCustomDownloadSite(currentVersion: String): UpdateInfo? {
        val raw = httpGet("$CUSTOM_BASE_URL/latest.json") ?: return null
        val data = JSONObject(raw)
        val version = data.optString("version")
        if (version.isEmpty()) return null
        if (compareVersions(version, currentVersion) <= 0) return null

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
        if (resolvedUrl.isEmpty()) return null

        // 解析发行说明：优先 releaseNotes，回退到 body
        val releaseNotes = when {
            data.has("releaseNotes") -> data.optString("releaseNotes")
            data.has("body") -> data.optString("body")
            else -> ""
        }

        return UpdateInfo(
            version = version,
            downloadUrl = resolvedUrl,
            releaseNotes = releaseNotes.ifEmpty { null }
        )
    }

    /**
     * 检测 GitHub Releases。
     *
     * 使用 /releases/latest 端点，GitHub 会自动返回最新的非 prerelease 发布。
     */
    private fun checkGitHubReleases(currentVersion: String): UpdateInfo? {
        val raw = httpGet(GITHUB_API) ?: return null
        val release = JSONObject(raw)
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