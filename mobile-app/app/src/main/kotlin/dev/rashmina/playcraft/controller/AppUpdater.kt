package dev.rashmina.playcraft.controller

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import kotlin.concurrent.thread

data class AndroidUpdateInfo(
    val currentVersion: String,
    val latestVersion: String,
    val hasUpdate: Boolean,
    val releaseUrl: String,
    val checkedAtMs: Long,
    val offlineOrError: Boolean,
    val statusMessage: String
)

/**
 * Silent daily background update checker targeting GitHub Releases API.
 * Author: Rashmina Fernando (https://www.rashmina.dev)
 */
object AppUpdater {
    const val RELEASES_API_URL = "https://api.github.com/repos/RashminaFdo/playcraft/releases/latest"
    const val RELEASES_PAGE_URL = "https://github.com/RashminaFdo/playcraft/releases/latest"
    private const val CHECK_INTERVAL_MS = 86_400_000L // 24 hours

    fun currentVersion(context: Context): String {
        return runCatching {
            val pInfo = context.packageManager.getPackageInfo(context.packageName, 0)
            pInfo.versionName ?: "1.0.0"
        }.getOrDefault("1.0.0")
    }

    fun isOnline(context: Context): Boolean {
        val cm = context.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return false
        val net = cm.activeNetwork ?: return false
        val caps = cm.getNetworkCapabilities(net) ?: return false
        return caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
    }

    fun isRemoteVersionNewer(current: String, remote: String): Boolean {
        val c = parseSemver(current)
        val r = parseSemver(remote)
        if (r.major != c.major) return r.major > c.major
        if (r.minor != c.minor) return r.minor > c.minor
        if (r.patch != c.patch) return r.patch > c.patch
        return c.prerelease && !r.prerelease
    }

    private data class SemVer(val major: Long, val minor: Long, val patch: Long, val prerelease: Boolean)

    private fun parseSemver(raw: String): SemVer {
        val clean = raw.trim().trimStart('v', 'V')
        val split = clean.split("-", limit = 2)
        val core = split[0].split(".").mapNotNull { it.toLongOrNull() }
        return SemVer(
            major = core.getOrElse(0) { 0L },
            minor = core.getOrElse(1) { 0L },
            patch = core.getOrElse(2) { 0L },
            prerelease = split.size > 1
        )
    }

    fun checkForUpdate(
        context: Context,
        config: ControllerConfig,
        force: Boolean,
        onComplete: (AndroidUpdateInfo) -> Unit
    ) {
        val appCtx = context.applicationContext
        val installed = currentVersion(appCtx)
        val nowMs = System.currentTimeMillis()
        val lastMs = config.lastUpdateCheckMs

        // Daily throttle for silent background checks unless user pressed "Check for Updates"
        if (!force && lastMs > 0L && (nowMs - lastMs) < CHECK_INTERVAL_MS) {
            val cachedTag = config.latestKnownVersionTag
            val hasCachedUpdate = cachedTag.isNotEmpty() && isRemoteVersionNewer(installed, cachedTag)
            val cachedUrl = config.latestReleaseUrl.ifBlank { RELEASES_PAGE_URL }
            onComplete(
                AndroidUpdateInfo(
                    currentVersion = installed,
                    latestVersion = if (cachedTag.isEmpty()) installed else cachedTag,
                    hasUpdate = hasCachedUpdate,
                    releaseUrl = cachedUrl,
                    checkedAtMs = lastMs,
                    offlineOrError = false,
                    statusMessage = if (hasCachedUpdate) "Update $cachedTag available on GitHub!" else "Up to date (v$installed)"
                )
            )
            return
        }

        if (!isOnline(appCtx)) {
            val cachedUrl = config.latestReleaseUrl.ifBlank { RELEASES_PAGE_URL }
            onComplete(
                AndroidUpdateInfo(
                    currentVersion = installed,
                    latestVersion = config.latestKnownVersionTag,
                    hasUpdate = false,
                    releaseUrl = cachedUrl,
                    checkedAtMs = lastMs,
                    offlineOrError = true,
                    statusMessage = "Offline: connect to the internet to check"
                )
            )
            return
        }

        thread(isDaemon = true, name = "playcraft-android-updater") {
            val result = runCatching {
                val conn = (URL(RELEASES_API_URL).openConnection() as HttpURLConnection).apply {
                    requestMethod = "GET"
                    connectTimeout = 6000
                    readTimeout = 8000
                    setRequestProperty("User-Agent", "PlayCraft-Android-Updater/1.0")
                    setRequestProperty("Accept", "application/vnd.github+json")
                }
                val code = conn.responseCode
                if (code == 404) {
                    conn.disconnect()
                    config.lastUpdateCheckMs = nowMs
                    return@runCatching AndroidUpdateInfo(
                        currentVersion = installed,
                        latestVersion = installed,
                        hasUpdate = false,
                        releaseUrl = RELEASES_PAGE_URL,
                        checkedAtMs = nowMs,
                        offlineOrError = false,
                        statusMessage = "Up to date (v$installed)"
                    )
                }
                if (code != 200) {
                    conn.disconnect()
                    throw IllegalStateException("GitHub API HTTP $code")
                }

                val body = conn.inputStream.bufferedReader().use { it.readText() }
                conn.disconnect()
                val json = JSONObject(body)
                val tag = json.optString("tag_name", "").trim()
                val htmlUrl = json.optString("html_url", RELEASES_PAGE_URL)
                    .takeIf { it.startsWith("https://") } ?: RELEASES_PAGE_URL
                if (tag.isEmpty()) {
                    throw IllegalStateException("Missing release tag")
                }

                config.lastUpdateCheckMs = nowMs
                config.latestKnownVersionTag = tag
                config.latestReleaseUrl = htmlUrl

                val hasUpdate = isRemoteVersionNewer(installed, tag)
                AndroidUpdateInfo(
                    currentVersion = installed,
                    latestVersion = tag,
                    hasUpdate = hasUpdate,
                    releaseUrl = htmlUrl,
                    checkedAtMs = nowMs,
                    offlineOrError = false,
                    statusMessage = if (hasUpdate) "Update $tag has arrived!" else "Up to date (v$installed)"
                )
            }.getOrElse { err ->
                AndroidUpdateInfo(
                    currentVersion = installed,
                    latestVersion = config.latestKnownVersionTag,
                    hasUpdate = false,
                    releaseUrl = config.latestReleaseUrl.ifBlank { RELEASES_PAGE_URL },
                    checkedAtMs = lastMs,
                    offlineOrError = true,
                    statusMessage = err.message ?: "Unable to reach GitHub"
                )
            }
            onComplete(result)
        }
    }
}

