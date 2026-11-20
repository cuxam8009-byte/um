package dev.dymith.browser

import android.net.Uri
import android.util.Log
import java.util.Locale
import java.util.concurrent.atomic.AtomicLong

/**
 * Cầu nối sang lõi Rust (libdymith_core.so).
 * Nếu thư viện không nạp được, hoặc đang dùng lõi dự phòng v0.0.1 (thiếu hàm mới),
 * ứng dụng vẫn chạy với bản rút gọn và màn hình chẩn đoán nói rõ lý do.
 */
object Core {
    private const val TAG = "DymithCore"
    const val DEFAULT_SEARCH = "https://www.google.com/search?q=%s"

    // Bit loại tài nguyên, khớp với core/src/adblock.rs
    const val T_SCRIPT = 1
    const val T_IMAGE = 2
    const val T_STYLE = 4
    const val T_FRAME = 8
    const val T_FONT = 64
    const val T_MEDIA = 32

    @Volatile
    var loaded: Boolean = false
        private set

    @Volatile
    var loadError: String? = null
        private set

    @Volatile
    var loadNanos: Long = 0L
        private set

    /** false khi thư viện là lõi dự phòng v0.0.1 (không có hàm mới). */
    @Volatile
    var full: Boolean = true
        private set

    // Số liệu đo đường nóng (mỗi request con đi qua JNI một lần).
    val requestsSeen = AtomicLong()
    val requestsBlocked = AtomicLong()
    val matchNanos = AtomicLong()

    fun load() {
        if (loaded) return
        val t0 = System.nanoTime()
        try {
            System.loadLibrary("dymith_core")
            loaded = true
        } catch (t: Throwable) {
            loadError = t.javaClass.simpleName + ": " + (t.message ?: "")
            Log.e(TAG, "Không nạp được lõi Rust", t)
        } finally {
            loadNanos = System.nanoTime() - t0
        }
    }

    private inline fun <T> guarded(fallback: T, block: () -> T): T {
        return try {
            block()
        } catch (e: UnsatisfiedLinkError) {
            full = false
            fallback
        } catch (t: Throwable) {
            Log.e(TAG, "Lỗi khi gọi lõi Rust", t)
            fallback
        }
    }

    fun version(): String {
        if (!loaded) return "CHƯA NẠP (" + (loadError ?: "?") + ")"
        return guarded("lỗi") { nativeVersion() }
    }

    /** Nạp bộ lọc + lịch sử từ thư mục dữ liệu của app. Gọi trên luồng nền. */
    fun init(filesDir: String): Int {
        if (!loaded) return -1
        return guarded(-1) { nativeInit(filesDir) }
    }

    fun resolve(input: String, template: String = DEFAULT_SEARCH): String {
        if (loaded) {
            val r = guarded<String?>(null) { nativeResolve(input, template) }
            if (r != null) return r
        }
        return fallbackResolve(input, template)
    }

    fun shouldBlock(url: String, source: String, type: Int): Boolean {
        if (!loaded) return false
        val t0 = System.nanoTime()
        val blocked = if (full) {
            guarded(false) { nativeShouldBlockEx(url, source, type) }
        } else {
            guarded(false) { nativeShouldBlock(url) }
        }
        matchNanos.addAndGet(System.nanoTime() - t0)
        requestsSeen.incrementAndGet()
        if (blocked) requestsBlocked.incrementAndGet()
        return blocked
    }

    fun recordVisit(url: String, title: String) {
        if (!loaded || !full) return
        guarded(Unit) { nativeRecordVisit(url, title, System.currentTimeMillis()) }
    }

    /** Gợi ý từ lịch sử: chuỗi JSON [{"t":"tiêu đề","u":"url"}, ...]. */
    fun suggest(query: String, limit: Int): String {
        if (!loaded || !full) return "[]"
        return guarded("[]") { nativeSuggest(query, limit, System.currentTimeMillis()) }
    }

    fun flush() {
        if (!loaded || !full) return
        guarded(Unit) { nativeFlush() }
    }

    fun clearHistory() {
        if (!loaded || !full) return
        guarded(Unit) { nativeClearHistory() }
    }

    fun stats(): String {
        if (!loaded) return "lõi chưa nạp"
        if (!full) return "lõi dự phòng v0.0.1 (không có thống kê)"
        return guarded("lỗi") { nativeStats() }
    }

    fun selfTest(): String {
        if (!loaded || !full) return "không có"
        return guarded("lỗi") { nativeSelfTest() }
    }

    fun reloadFilters(filesDir: String): String {
        if (!loaded || !full) return "không hỗ trợ"
        return guarded("lỗi") { nativeReloadFilters(filesDir) }
    }

    /** Đoán loại tài nguyên từ đuôi tệp / header Accept (WebView không cho biết loại thật). */
    fun typeBits(url: String, accept: String?): Int {
        var end = url.indexOf('?')
        if (end < 0) end = url.indexOf('#')
        if (end < 0) end = url.length
        val path = url.substring(0, end).lowercase(Locale.ROOT)
        val dot = path.lastIndexOf('.')
        val ext = if (dot >= 0 && dot > path.lastIndexOf('/')) path.substring(dot + 1) else ""
        return when (ext) {
            "js", "mjs" -> T_SCRIPT
            "css" -> T_STYLE
            "png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "avif", "bmp" -> T_IMAGE
            "woff", "woff2", "ttf", "otf" -> T_FONT
            "mp4", "webm", "m3u8", "mp3", "ogg", "m4s" -> T_MEDIA
            else -> typeFromAccept(accept)
        }
    }

    private fun typeFromAccept(accept: String?): Int {
        if (accept == null) return 0
        return when {
            accept.contains("image/") -> T_IMAGE
            accept.contains("text/css") -> T_STYLE
            accept.contains("text/html") -> T_FRAME
            else -> 0
        }
    }

    private fun fallbackResolve(input: String, template: String): String {
        val s = input.trim()
        if (s.isEmpty()) return "about:blank"
        if (s.startsWith("http://") || s.startsWith("https://")) return s
        return template.replace("%s", Uri.encode(s))
    }

    // Có trong cả lõi mới và lõi dự phòng
    @JvmStatic
    external fun nativeVersion(): String

    @JvmStatic
    external fun nativeResolve(input: String, template: String): String

    @JvmStatic
    external fun nativeShouldBlock(url: String): Boolean

    // Chỉ có trong lõi mới (v0.0.2 trở lên)
    @JvmStatic
    external fun nativeShouldBlockEx(url: String, source: String, type: Int): Boolean

    @JvmStatic
    external fun nativeInit(filesDir: String): Int

    @JvmStatic
    external fun nativeRecordVisit(url: String, title: String, nowMs: Long)

    @JvmStatic
    external fun nativeSuggest(query: String, limit: Int, nowMs: Long): String

    @JvmStatic
    external fun nativeFlush()

    @JvmStatic
    external fun nativeClearHistory()

    @JvmStatic
    external fun nativeStats(): String

    @JvmStatic
    external fun nativeReloadFilters(filesDir: String): String

    @JvmStatic
    external fun nativeSelfTest(): String
}
