package dev.dymith.browser

import android.app.Activity
import android.app.ActivityManager
import android.content.Context
import android.os.Build
import android.os.Debug
import android.os.Handler
import android.os.HandlerThread
import android.provider.Settings
import android.system.Os
import android.system.OsConstants
import android.view.FrameMetrics
import android.view.Window
import android.webkit.WebView
import java.util.Locale

/** Thu thời gian dựng khung hình (frame time) của cửa sổ chính để đo độ mượt. */
class FrameStats(private val window: Window) {
    private val thread = HandlerThread("dymith-frames")
    private val ring = IntArray(1200) // micro giây, vòng tròn
    private var idx = 0
    private var total = 0L
    private var over1 = 0L
    private var over2 = 0L
    private var running = false

    @Volatile
    var budgetUs: Int = 16_667

    private val listener = Window.OnFrameMetricsAvailableListener { _, metrics, _ ->
        val us = (metrics.getMetric(FrameMetrics.TOTAL_DURATION) / 1000L).toInt()
        synchronized(this) {
            ring[idx] = us
            idx = (idx + 1) % ring.size
            total++
            if (us > budgetUs) over1++
            if (us > budgetUs * 2) over2++
            Unit
        }
    }

    fun start() {
        if (running) return
        thread.start()
        window.addOnFrameMetricsAvailableListener(listener, Handler(thread.looper))
        running = true
    }

    fun stop() {
        if (!running) return
        window.removeOnFrameMetricsAvailableListener(listener)
        thread.quitSafely()
        running = false
    }

    fun summary(): String {
        var n = 0
        var copy = IntArray(0)
        var t = 0L
        var o1 = 0L
        var o2 = 0L
        synchronized(this) {
            n = minOf(total, ring.size.toLong()).toInt()
            copy = ring.copyOf(n)
            t = total
            o1 = over1
            o2 = over2
        }
        if (n == 0) return "chưa có khung hình nào"
        copy.sort()
        val avg = copy.sumOf { it.toLong() }.toDouble() / n / 1000.0
        fun pct(p: Double): Double = copy[minOf(n - 1, (p * n).toInt())] / 1000.0
        val budgetMs = budgetUs / 1000.0
        return String.format(
            Locale.US,
            "%d khung (cửa sổ %d) | TB %.1f ms | p50 %.1f | p90 %.1f | p99 %.1f | " +
                "vượt %.1f ms: %d (%.1f%%) | vượt gấp đôi: %d",
            t, n, avg, pct(0.5), pct(0.9), pct(0.99), budgetMs, o1, 100.0 * o1 / t, o2
        )
    }
}

object Diagnostics {

    @Suppress("DEPRECATION")
    fun report(
        activity: Activity,
        stats: FrameStats?,
        extras: List<Pair<String, String>>
    ): String {
        val sb = StringBuilder()
        fun add(k: String, v: String) {
            sb.append(k).append(": ").append(v).append('\n')
        }

        add("Dymith", "Phase 0.2 - báo cáo chẩn đoán")
        add("Lõi Rust", Core.version())
        add("Nạp lõi Rust", String.format(Locale.US, "%.2f ms", Core.loadNanos / 1e6))

        add("Thiết bị", Build.MANUFACTURER + " " + Build.MODEL + " (" + Build.DEVICE + ")")
        add("Android", Build.VERSION.RELEASE + " (API " + Build.VERSION.SDK_INT + "), vá bảo mật " + Build.VERSION.SECURITY_PATCH)
        add("ABI", Build.SUPPORTED_ABIS.joinToString())
        val soc = if (Build.VERSION.SDK_INT >= 31) " / " + Build.SOC_MODEL else ""
        add("Phần cứng", Build.HARDWARE + soc)
        add("Kích thước trang bộ nhớ", (Os.sysconf(OsConstants._SC_PAGESIZE) / 1024).toString() + " KB")

        val am = activity.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
        val mi = ActivityManager.MemoryInfo()
        am.getMemoryInfo(mi)
        add("RAM máy", (mi.totalMem / (1024L * 1024L)).toString() + " MB, trống " + (mi.availMem / (1024L * 1024L)) + " MB, máy RAM thấp: " + am.isLowRamDevice)

        val dm = Debug.MemoryInfo()
        Debug.getMemoryInfo(dm)
        add(
            "Bộ nhớ tiến trình vỏ (PSS)",
            (dm.totalPss / 1024).toString() + " MB (native " + (dm.nativePss / 1024) +
                ", dalvik " + (dm.dalvikPss / 1024) + ", khác " + (dm.otherPss / 1024) + ")"
        )

        val wv = WebView.getCurrentWebViewPackage()
        add("WebView", (wv?.packageName ?: "?") + " " + (wv?.versionName ?: "?"))

        val hz = activity.windowManager.defaultDisplay.refreshRate
        add("Tần số quét", String.format(Locale.US, "%.0f Hz", hz))

        val ime = Settings.Secure.getString(activity.contentResolver, Settings.Secure.DEFAULT_INPUT_METHOD)
        add("Bàn phím", ime ?: "?")

        val seen = Core.requestsSeen.get()
        val avgNs = if (seen > 0) Core.matchNanos.get() / seen else 0L
        add(
            "Chặn quảng cáo (JNI)",
            "$seen request con, chặn " + Core.requestsBlocked.get() + ", TB " + avgNs +
                " ns/lần gọi (đo ở Kotlin, gồm cả chuyển chuỗi)"
        )
        add("Thống kê lõi", Core.stats())
        add("Tự kiểm tra bộ lọc", Core.selfTest())

        for ((k, v) in extras) add(k, v)

        add("Độ mượt", stats?.summary() ?: "không có số liệu")
        return sb.toString()
    }
}
