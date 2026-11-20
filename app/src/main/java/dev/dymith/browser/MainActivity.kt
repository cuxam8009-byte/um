package dev.dymith.browser

import android.annotation.SuppressLint
import android.app.Activity
import android.app.AlertDialog
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.os.Process
import android.os.SystemClock
import android.text.InputType
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.KeyEvent
import android.view.View
import android.view.ViewGroup
import android.view.ViewTreeObserver
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebChromeClient
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.ImageButton
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.PopupMenu
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.webkit.JavaScriptReplyProxy
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONObject
import java.io.ByteArrayInputStream
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.util.Locale
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/** Bảng màu đen-trắng của giao diện mới (khớp với biến CSS trong assets/start.html). */
internal class Palette(
    val bg: Int,
    val ink: Int,
    val mute: Int,
    val surf: Int,
    val line: Int,
    val acc: Int
) {
    companion object {
        val LIGHT = Palette(
            0xFFFFFFFF.toInt(), 0xFF000000.toInt(), 0xFF737373.toInt(),
            0xFFF5F5F5.toInt(), 0xFFE5E5E5.toInt(), 0xFF000000.toInt()
        )
        val DARK = Palette(
            0xFF000000.toInt(), 0xFFFFFFFF.toInt(), 0xFFA3A3A3.toInt(),
            0xFF121212.toInt(), 0xFF2A2A2A.toInt(), 0xFFFFFFFF.toInt()
        )
    }
}

/**
 * Dymith Phase 0.2: một tab, WebView hệ thống + lõi Rust.
 * Trang chủ là HTML (assets/start.html) tự có thanh tìm kiếm; thanh công cụ native phẳng chỉ hiện
 * khi đang xem một website. Không còn blur, bóng đổ nặng hay nền gradient cố định (nguyên nhân lag).
 */
class MainActivity : Activity() {

    private companion object {
        const val PREF_ENGINE = "engine_tpl"
        const val PREF_THEME = "chrome_theme"
        const val PREF_KB_TEXT = "kb_text"
        const val PREF_UA_CLEAN = "ua_clean"
        const val MATCH = ViewGroup.LayoutParams.MATCH_PARENT
    }

    private lateinit var prefs: SharedPreferences
    private lateinit var root: FrameLayout
    private lateinit var web: WebView
    private lateinit var progressBar: View
    private lateinit var bar: FrameLayout
    private lateinit var omnibox: EditText
    private lateinit var btnBack: ImageButton
    private lateinit var btnReload: ImageButton
    private lateinit var btnMenu: ImageButton

    private val worker: ExecutorService = Executors.newSingleThreadExecutor()
    private var frameStats: FrameStats? = null

    @Volatile
    private var currentUrl: String = ""
    private var defaultUa: String = ""
    private var searchTemplate: String = Core.DEFAULT_SEARCH
    private var chromeMode: String = "auto"

    private var loading = false
    private var onStartPage = true
    private var scrollHidden = false
    private var imeVisible = false
    private var insetTop = 0
    private var insetLeft = 0
    private var insetRight = 0
    private var navBottom = 0
    private var imeBottom = 0

    private var firstDrawMs = -1L
    private var onCreateMs = 0.0
    private var webViewCreateMs = 0.0

    // ------------------------------------------------------------------ vòng đời

    override fun onCreate(savedInstanceState: Bundle?) {
        val t0 = SystemClock.elapsedRealtimeNanos()
        super.onCreate(savedInstanceState)
        prefs = getSharedPreferences("dymith", MODE_PRIVATE)
        searchTemplate = prefs.getString(PREF_ENGINE, null) ?: Core.DEFAULT_SEARCH
        chromeMode = prefs.getString(PREF_THEME, null) ?: "auto"
        Core.load()
        val dataDir = filesDir.absolutePath
        worker.execute { Core.init(dataDir) }

        WindowCompat.setDecorFitsSystemWindows(window, false)
        window.statusBarColor = Color.TRANSPARENT
        window.navigationBarColor = Color.TRANSPARENT

        buildUi()
        setContentView(root)
        applyChromeTheme()
        applyUserAgentMode()
        setupBridge()

        val stats = FrameStats(window)
        stats.budgetUs = budgetFromDisplay()
        stats.start()
        frameStats = stats

        root.viewTreeObserver.addOnPreDrawListener(object : ViewTreeObserver.OnPreDrawListener {
            override fun onPreDraw(): Boolean {
                root.viewTreeObserver.removeOnPreDrawListener(this)
                firstDrawMs = SystemClock.elapsedRealtime() - Process.getStartElapsedRealtime()
                return true
            }
        })

        if (savedInstanceState != null) {
            web.restoreState(savedInstanceState)
        } else if (!handleIntent(intent)) {
            loadStartPage()
        }
        onCreateMs = (SystemClock.elapsedRealtimeNanos() - t0) / 1_000_000.0
    }

    override fun onNewIntent(intent: Intent?) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    override fun onResume() {
        super.onResume()
        web.onResume()
    }

    override fun onPause() {
        web.onPause()
        flushAsync()
        super.onPause()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        web.saveState(outState)
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        applyChromeTheme()
    }

    override fun onDestroy() {
        frameStats?.stop()
        flushAsync()
        worker.shutdown()
        root.removeView(web)
        web.destroy()
        super.onDestroy()
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        if (omnibox.hasFocus()) {
            omnibox.clearFocus()
            hideKeyboard()
        } else if (web.canGoBack()) {
            web.goBack()
        } else {
            super.onBackPressed()
        }
    }

    private fun flushAsync() {
        try {
            worker.execute { Core.flush() }
        } catch (e: Exception) {
            Log.w("Dymith", "flush bỏ qua: " + e)
        }
    }

    private fun handleIntent(i: Intent?): Boolean {
        if (i == null || i.action != Intent.ACTION_VIEW) return false
        val data = i.data ?: return false
        web.loadUrl(data.toString())
        return true
    }

    // ------------------------------------------------------------------ tiện ích

    private fun dp(v: Int): Int = (v * resources.displayMetrics.density + 0.5f).toInt()

    private fun toast(msg: String) {
        Toast.makeText(this, msg, Toast.LENGTH_SHORT).show()
    }

    private fun budgetFromDisplay(): Int {
        @Suppress("DEPRECATION")
        val hz = windowManager.defaultDisplay.refreshRate
        return if (hz > 1f) (1_000_000f / hz).toInt() else 16_667
    }

    private fun hideKeyboard() {
        val imm = getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        imm.hideSoftInputFromWindow(omnibox.windowToken, 0)
    }

    // ------------------------------------------------------------------ giao diện

    @SuppressLint("SetJavaScriptEnabled")
    private fun buildUi() {
        root = FrameLayout(this)

        val tw = SystemClock.elapsedRealtimeNanos()
        web = WebView(this)
        webViewCreateMs = (SystemClock.elapsedRealtimeNanos() - tw) / 1_000_000.0
        configureWebView()
        root.addView(web, FrameLayout.LayoutParams(MATCH, MATCH))

        progressBar = View(this)
        progressBar.pivotX = 0f
        progressBar.scaleX = 0f
        progressBar.visibility = View.INVISIBLE
        val progressParams = FrameLayout.LayoutParams(MATCH, dp(2))
        progressParams.gravity = Gravity.TOP
        root.addView(progressBar, progressParams)

        // Thanh công cụ dạng viên thuốc, nổi ở đáy màn hình; chỉ hiện khi đang xem website.
        bar = FrameLayout(this)
        btnBack = iconButton(R.drawable.ic_back, "Quay lại") {
            if (web.canGoBack()) web.goBack()
        }
        btnReload = iconButton(R.drawable.ic_refresh, "Tải lại") {
            if (loading) web.stopLoading() else web.reload()
        }
        btnMenu = iconButton(R.drawable.ic_more, "Menu") { anchor -> showMenu(anchor) }
        omnibox = buildOmnibox()

        val row = LinearLayout(this)
        row.orientation = LinearLayout.HORIZONTAL
        row.gravity = Gravity.CENTER_VERTICAL
        row.setPadding(dp(8), 0, dp(8), 0)
        row.addView(btnBack, LinearLayout.LayoutParams(dp(44), dp(44)))
        row.addView(omnibox, LinearLayout.LayoutParams(0, dp(44), 1f))
        row.addView(btnReload, LinearLayout.LayoutParams(dp(44), dp(44)))
        row.addView(btnMenu, LinearLayout.LayoutParams(dp(44), dp(44)))
        bar.addView(row, FrameLayout.LayoutParams(MATCH, MATCH))
        bar.visibility = View.GONE

        val barParams = FrameLayout.LayoutParams(MATCH, dp(56))
        barParams.gravity = Gravity.BOTTOM
        barParams.setMargins(dp(12), 0, dp(12), dp(12))
        root.addView(bar, barParams)

        ViewCompat.setOnApplyWindowInsetsListener(root) { _, insets ->
            val sys = insets.getInsets(
                WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout()
            )
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
            insetTop = sys.top
            insetLeft = sys.left
            insetRight = sys.right
            navBottom = sys.bottom
            imeBottom = ime.bottom
            imeVisible = insets.isVisible(WindowInsetsCompat.Type.ime())
            applyInsets()
            updateBar()
            insets
        }
        updateBackButton()
    }

    /** Chỉ đụng tới layout khi số đo thực sự đổi, để tránh dựng lại trang web vô ích. */
    private fun applyInsets() {
        // Khi bàn phím mở cho ô nhập CỦA TRANG, thu nhỏ WebView để ô nhập không bị che.
        // Khi gõ ở ô địa chỉ native thì để WebView yên (thanh nổi lên trên bàn phím).
        val webBottom = if (imeVisible && !omnibox.hasFocus()) imeBottom else navBottom
        val barBottom = dp(12) + (if (imeVisible) imeBottom else navBottom)

        val wlp = web.layoutParams as FrameLayout.LayoutParams
        if (wlp.topMargin != insetTop || wlp.bottomMargin != webBottom) {
            wlp.topMargin = insetTop
            wlp.bottomMargin = webBottom
            web.layoutParams = wlp
        }
        val plp = progressBar.layoutParams as FrameLayout.LayoutParams
        if (plp.topMargin != insetTop) {
            plp.topMargin = insetTop
            progressBar.layoutParams = plp
        }
        val blp = bar.layoutParams as FrameLayout.LayoutParams
        val left = dp(12) + insetLeft
        val right = dp(12) + insetRight
        if (blp.bottomMargin != barBottom || blp.leftMargin != left || blp.rightMargin != right) {
            blp.bottomMargin = barBottom
            blp.leftMargin = left
            blp.rightMargin = right
            bar.layoutParams = blp
        }
    }

    private fun iconButton(res: Int, desc: String, onClick: (View) -> Unit): ImageButton {
        val b = ImageButton(this)
        b.setImageResource(res)
        b.setBackgroundColor(Color.TRANSPARENT)
        b.scaleType = ImageView.ScaleType.CENTER_INSIDE
        b.setPadding(dp(11), dp(11), dp(11), dp(11))
        b.contentDescription = desc
        b.setOnClickListener { v -> onClick(v) }
        return b
    }

    private fun omniboxInputType(): Int {
        return if (prefs.getBoolean(PREF_KB_TEXT, false)) {
            InputType.TYPE_CLASS_TEXT
        } else {
            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
        }
    }

    private fun buildOmnibox(): EditText {
        val e = EditText(this)
        e.setSingleLine(true)
        e.hint = "Tìm kiếm hoặc nhập địa chỉ"
        e.setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
        e.background = null
        e.gravity = Gravity.CENTER_VERTICAL
        e.setPadding(dp(8), 0, dp(8), 0)
        e.imeOptions = EditorInfo.IME_ACTION_GO or EditorInfo.IME_FLAG_NO_EXTRACT_UI
        e.inputType = omniboxInputType()
        e.setSelectAllOnFocus(true)
        e.setOnEditorActionListener { v, actionId, event ->
            val enter = event != null &&
                event.keyCode == KeyEvent.KEYCODE_ENTER &&
                event.action == KeyEvent.ACTION_DOWN
            if (actionId == EditorInfo.IME_ACTION_GO || actionId == EditorInfo.IME_ACTION_DONE || enter) {
                go(v.text.toString())
                true
            } else {
                false
            }
        }
        e.setOnFocusChangeListener { _, hasFocus ->
            if (hasFocus) {
                scrollHidden = false
                updateBar()
            } else {
                applyInsets()
            }
        }
        return e
    }

    private fun isNight(): Boolean {
        return when (chromeMode) {
            "dark" -> true
            "light" -> false
            else -> (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
                Configuration.UI_MODE_NIGHT_YES
        }
    }

    /** Áp bảng màu cho phần chrome native (nền, thanh, biểu tượng, thanh trạng thái). */
    private fun applyChromeTheme() {
        val night = isNight()
        val p = if (night) Palette.DARK else Palette.LIGHT
        root.setBackgroundColor(p.bg)
        window.setBackgroundDrawable(ColorDrawable(p.bg))
        web.setBackgroundColor(p.bg)

        val d = GradientDrawable()
        d.shape = GradientDrawable.RECTANGLE
        d.cornerRadius = dp(28).toFloat()
        d.setColor(p.surf)
        d.setStroke(dp(1), p.line)
        bar.background = d
        bar.elevation = if (night) 0f else dp(6).toFloat()

        val tintList = ColorStateList.valueOf(p.ink)
        btnBack.imageTintList = tintList
        btnReload.imageTintList = tintList
        btnMenu.imageTintList = tintList
        omnibox.setTextColor(p.ink)
        omnibox.setHintTextColor(p.mute)
        progressBar.setBackgroundColor(p.acc)

        val controller = WindowCompat.getInsetsController(window, window.decorView)
        controller.isAppearanceLightStatusBars = !night
        controller.isAppearanceLightNavigationBars = !night
    }

    private fun setChromeMode(mode: String) {
        val m = if (mode == "light" || mode == "dark") mode else "auto"
        if (m == chromeMode) return
        chromeMode = m
        prefs.edit().putString(PREF_THEME, m).apply()
        applyChromeTheme()
    }

    private fun hiddenOffset(): Float {
        val lp = bar.layoutParams as FrameLayout.LayoutParams
        return (dp(56) + lp.bottomMargin + dp(8)).toFloat()
    }

    /** Một chỗ duy nhất quyết định thanh native hiện/ẩn/trượt. */
    private fun updateBar() {
        val hideForPageKeyboard = imeVisible && !omnibox.hasFocus()
        if (onStartPage || hideForPageKeyboard) {
            if (bar.visibility != View.GONE) {
                bar.animate().cancel()
                bar.visibility = View.GONE
            }
            return
        }
        if (bar.visibility != View.VISIBLE) bar.visibility = View.VISIBLE
        val target = if (scrollHidden && !omnibox.hasFocus()) hiddenOffset() else 0f
        if (bar.translationY != target) {
            bar.animate().translationY(target).setDuration(160).withLayer().start()
        }
    }

    private fun onWebScrolled(dy: Int) {
        if (omnibox.hasFocus() || onStartPage) return
        if (dy > dp(4) && !scrollHidden) {
            scrollHidden = true
            updateBar()
        } else if (dy < -dp(4) && scrollHidden) {
            scrollHidden = false
            updateBar()
        }
    }

    private fun updateBackButton() {
        btnBack.alpha = if (web.canGoBack()) 1f else 0.35f
    }

    private fun setLoadingUi(isLoading: Boolean) {
        loading = isLoading
        btnReload.setImageResource(if (isLoading) R.drawable.ic_close else R.drawable.ic_refresh)
        if (!isLoading) progressBar.visibility = View.INVISIBLE
    }

    private fun setStartFlag(url: String) {
        val s = StartPage.isStartUrl(url)
        if (s != onStartPage) {
            onStartPage = s
            scrollHidden = false
            updateBar()
        }
    }

    // ------------------------------------------------------------------ WebView

    @SuppressLint("SetJavaScriptEnabled")
    private fun configureWebView() {
        val s = web.settings
        s.javaScriptEnabled = true
        s.domStorageEnabled = true
        s.allowFileAccess = false
        s.allowContentAccess = false
        s.setSupportMultipleWindows(false)
        s.useWideViewPort = true
        s.loadWithOverviewMode = true
        s.setSupportZoom(true)
        s.builtInZoomControls = true
        s.displayZoomControls = false
        s.mediaPlaybackRequiresUserGesture = true
        s.mixedContentMode = WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE
        s.safeBrowsingEnabled = true
        defaultUa = s.userAgentString ?: ""

        web.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView?, request: WebResourceRequest?): Boolean {
                val scheme = request?.url?.scheme?.lowercase(Locale.ROOT) ?: return false
                return if (scheme == "http" || scheme == "https" || scheme == "about" ||
                    scheme == "data" || scheme == "blob"
                ) {
                    false
                } else {
                    toast("Phase 0 chưa mở liên kết kiểu " + scheme)
                    true
                }
            }

            override fun shouldInterceptRequest(view: WebView?, request: WebResourceRequest?): WebResourceResponse? {
                if (request == null) return null
                val uri = request.url
                if (uri.host == StartPage.HOST) {
                    return startPageResponse(uri.path)
                }
                if (request.isForMainFrame) return null
                val url = uri.toString()
                if (!url.startsWith("http")) return null
                val type = Core.typeBits(url, request.requestHeaders?.get("Accept"))
                return if (Core.shouldBlock(url, currentUrl, type)) emptyResponse() else null
            }

            override fun onPageStarted(view: WebView?, url: String?, favicon: Bitmap?) {
                val u = url ?: ""
                currentUrl = u
                setStartFlag(u)
                setLoadingUi(true)
                if (!omnibox.hasFocus()) omnibox.setText(displayUrl(u))
            }

            override fun onPageFinished(view: WebView?, url: String?) {
                setLoadingUi(false)
                updateBackButton()
                val u = url ?: return
                if (!StartPage.isStartUrl(u) && u.startsWith("http")) {
                    val title = view?.title ?: ""
                    try {
                        worker.execute { Core.recordVisit(u, title) }
                    } catch (e: Exception) {
                        Log.w("Dymith", "recordVisit bỏ qua: " + e)
                    }
                }
            }

            override fun doUpdateVisitedHistory(view: WebView?, url: String?, isReload: Boolean) {
                updateBackButton()
                val u = url ?: return
                currentUrl = u
                setStartFlag(u)
                if (!omnibox.hasFocus()) omnibox.setText(displayUrl(u))
            }

            override fun onRenderProcessGone(view: WebView?, detail: RenderProcessGoneDetail?): Boolean {
                toast("Tiến trình hiển thị đã dừng, đang khởi động lại")
                recreate()
                return true
            }
        }

        web.webChromeClient = object : WebChromeClient() {
            override fun onProgressChanged(view: WebView?, newProgress: Int) {
                progressBar.visibility = if (newProgress in 1..99) View.VISIBLE else View.INVISIBLE
                progressBar.scaleX = newProgress / 100f
            }
        }

        web.setDownloadListener { _, _, _, _, _ ->
            toast("Tải xuống sẽ có ở Phase 1")
        }

        web.setOnScrollChangeListener { _, _, scrollY, _, oldScrollY ->
            onWebScrolled(scrollY - oldScrollY)
        }
    }

    private fun startPageResponse(path: String?): WebResourceResponse? {
        if (path != null && path != "" && path != "/") {
            return WebResourceResponse(
                "text/plain", "utf-8", 404, "Not Found",
                mapOf("Cache-Control" to "no-store"), ByteArrayInputStream(ByteArray(0))
            )
        }
        return try {
            WebResourceResponse(
                "text/html", "utf-8", 200, "OK",
                mapOf("Cache-Control" to "no-store"), assets.open("start.html")
            )
        } catch (t: Throwable) {
            Log.e("Dymith", "Không đọc được start.html", t)
            null
        }
    }

    private fun emptyResponse(): WebResourceResponse {
        return WebResourceResponse("text/plain", "utf-8", ByteArrayInputStream(ByteArray(0)))
    }

    private fun displayUrl(url: String): String {
        if (StartPage.isStartUrl(url) || url == "about:blank") return ""
        return url.removePrefix("https://").removePrefix("http://").removeSuffix("/")
    }

    private fun loadStartPage() {
        web.loadUrl(StartPage.URL)
        omnibox.setText("")
    }

    private fun go(raw: String, template: String = searchTemplate) {
        val url = Core.resolve(raw, template)
        omnibox.clearFocus()
        hideKeyboard()
        if (url == "about:blank") loadStartPage() else web.loadUrl(url)
    }

    // ------------------------------------------------------------------ cầu nối với trang chủ

    /**
     * Cầu nối "Browser" chỉ được tiêm vào origin của trang chủ (addWebMessageListener giới hạn
     * origin), nên website bên ngoài không thể gọi và không thể đọc lịch sử duyệt web.
     */
    private fun setupBridge() {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) {
            toast("WebView quá cũ nên trang chủ mới không hoạt động. Hãy cập nhật Android System WebView.")
            return
        }
        WebViewCompat.addWebMessageListener(web, "Browser", setOf(StartPage.ORIGIN)) { _, message, _, isMainFrame, replyProxy ->
            if (isMainFrame) {
                handleStartMessage(message.data, replyProxy)
            }
        }
    }

    private fun handleStartMessage(raw: String?, reply: JavaScriptReplyProxy) {
        if (raw == null) return
        val o = try {
            JSONObject(raw)
        } catch (t: Throwable) {
            return
        }
        when (o.optString("op")) {
            "open" -> {
                val url = o.optString("url")
                if (url.startsWith("https://") || url.startsWith("http://")) web.loadUrl(url)
            }
            "go" -> {
                val text = o.optString("text")
                val tpl = o.optString("tpl")
                val template = if (tpl.startsWith("https://") && tpl.contains("%s")) tpl else searchTemplate
                if (text.isNotBlank()) go(text, template)
            }
            "suggest" -> {
                val id = o.optInt("id")
                val items = Core.suggest(o.optString("q"), 5)
                reply.postMessage("{\"op\":\"suggest\",\"id\":" + id + ",\"items\":" + items + "}")
            }
            "theme" -> setChromeMode(o.optString("mode"))
            "engine" -> {
                val tpl = o.optString("tpl")
                if (tpl.startsWith("https://") && tpl.contains("%s") && tpl != searchTemplate) {
                    searchTemplate = tpl
                    prefs.edit().putString(PREF_ENGINE, tpl).apply()
                }
            }
        }
    }

    // ------------------------------------------------------------------ UA, menu, chẩn đoán

    private fun cleanUserAgent(ua: String): String {
        var out = ua.replace("; wv)", ")")
        out = out.replace(Regex("Version/\\d+(\\.\\d+)*\\s"), "")
        return out + " Dymith/0.0.2"
    }

    private fun applyUserAgentMode() {
        val clean = prefs.getBoolean(PREF_UA_CLEAN, false)
        web.settings.userAgentString = if (clean) cleanUserAgent(defaultUa) else defaultUa
    }

    private fun showMenu(anchor: View) {
        val pm = PopupMenu(this, anchor)
        val m = pm.menu
        m.add(0, 1, 0, "Trang chủ")
        m.add(0, 2, 1, "Cập nhật bộ lọc quảng cáo")
        m.add(0, 3, 2, "Xóa lịch sử duyệt web")
        val kbItem = m.add(0, 4, 3, "Bàn phím ô địa chỉ: văn bản")
        kbItem.isCheckable = true
        kbItem.isChecked = prefs.getBoolean(PREF_KB_TEXT, false)
        val uaItem = m.add(0, 5, 4, "UA chuẩn trình duyệt (bỏ wv)")
        uaItem.isCheckable = true
        uaItem.isChecked = prefs.getBoolean(PREF_UA_CLEAN, false)
        m.add(0, 6, 5, "Chẩn đoán và báo cáo")

        pm.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                1 -> loadStartPage()
                2 -> updateFilters()
                3 -> clearHistory()
                4 -> toggleKeyboardMode()
                5 -> toggleUserAgent()
                6 -> showDiagnostics()
            }
            true
        }
        pm.show()
    }

    private fun clearHistory() {
        try {
            worker.execute { Core.clearHistory() }
            toast("Đã xóa lịch sử duyệt web")
        } catch (e: Exception) {
            Log.w("Dymith", "clearHistory bỏ qua: " + e)
        }
    }

    private fun toggleKeyboardMode() {
        val now = !prefs.getBoolean(PREF_KB_TEXT, false)
        prefs.edit().putBoolean(PREF_KB_TEXT, now).apply()
        omnibox.inputType = omniboxInputType()
        val imm = getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        imm.restartInput(omnibox)
        toast(if (now) "Bàn phím văn bản" else "Bàn phím địa chỉ (URL)")
    }

    private fun toggleUserAgent() {
        val now = !prefs.getBoolean(PREF_UA_CLEAN, false)
        prefs.edit().putBoolean(PREF_UA_CLEAN, now).apply()
        applyUserAgentMode()
        web.reload()
        toast(if (now) "UA chuẩn trình duyệt: bật" else "UA mặc định của WebView")
    }

    // Tải EasyList + EasyPrivacy về filesDir/filters rồi để lõi Rust dựng lại bộ lọc.
    private fun updateFilters() {
        toast("Đang tải bộ lọc quảng cáo...")
        val base = filesDir
        worker.execute {
            val dir = File(base, "filters")
            dir.mkdirs()
            val lists = arrayOf(
                Pair("easylist.txt", "https://easylist.to/easylist/easylist.txt"),
                Pair("easyprivacy.txt", "https://easylist.to/easylist/easyprivacy.txt")
            )
            var ok = 0
            for (l in lists) {
                if (downloadTo(l.second, File(dir, l.first))) ok++
            }
            val stats = if (ok > 0) Core.reloadFilters(base.absolutePath) else ""
            Log.i("Dymith", "Bộ lọc: " + stats)
            runOnUiThread {
                toast(
                    if (ok > 0) "Đã cập nhật " + ok + "/" + lists.size + " danh sách bộ lọc"
                    else "Không tải được bộ lọc (kiểm tra mạng)"
                )
            }
        }
    }

    private fun downloadTo(urlStr: String, dest: File): Boolean {
        var conn: HttpURLConnection? = null
        try {
            val c = URL(urlStr).openConnection() as HttpURLConnection
            conn = c
            c.connectTimeout = 15000
            c.readTimeout = 30000
            c.instanceFollowRedirects = true
            if (c.responseCode != 200) return false
            val tmp = File(dest.parentFile, dest.name + ".tmp")
            c.inputStream.use { input ->
                tmp.outputStream().use { out -> input.copyTo(out) }
            }
            if (tmp.length() < 1024L) {
                tmp.delete()
                return false
            }
            dest.delete()
            return tmp.renameTo(dest)
        } catch (t: Throwable) {
            Log.e("Dymith", "Tải bộ lọc lỗi: " + t)
            return false
        } finally {
            conn?.disconnect()
        }
    }

    private fun showDiagnostics() {
        val firstDraw = if (firstDrawMs >= 0) firstDrawMs.toString() + " ms" else "chưa đo"
        val extras = ArrayList<Pair<String, String>>()
        extras.add(Pair("Khung đầu tiên (tính từ lúc tiến trình bắt đầu)", firstDraw))
        extras.add(Pair("Activity.onCreate", String.format(Locale.US, "%.1f ms", onCreateMs)))
        extras.add(Pair("Tạo WebView", String.format(Locale.US, "%.1f ms", webViewCreateMs)))
        extras.add(Pair("Giao diện chrome", chromeMode + (if (isNight()) " (đang tối)" else " (đang sáng)")))
        extras.add(Pair("Công cụ tìm kiếm", searchTemplate))
        extras.add(Pair("UA chuẩn trình duyệt", prefs.getBoolean(PREF_UA_CLEAN, false).toString()))
        extras.add(Pair("Bàn phím ô địa chỉ", if (prefs.getBoolean(PREF_KB_TEXT, false)) "văn bản" else "URL"))
        extras.add(Pair("User-Agent", web.settings.userAgentString ?: "?"))

        val text = Diagnostics.report(this, frameStats, extras)

        val tv = TextView(this)
        tv.text = text
        tv.typeface = Typeface.MONOSPACE
        tv.textSize = 11f
        tv.setTextIsSelectable(true)
        tv.setPadding(dp(16), dp(8), dp(16), dp(8))
        val sv = ScrollView(this)
        sv.addView(tv)

        AlertDialog.Builder(this)
            .setTitle("Chẩn đoán Dymith")
            .setView(sv)
            .setPositiveButton("Sao chép") { _, _ ->
                val cm = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                cm.setPrimaryClip(ClipData.newPlainText("Dymith report", text))
                toast("Đã sao chép báo cáo")
            }
            .setNegativeButton("Đóng", null)
            .show()
    }
}
