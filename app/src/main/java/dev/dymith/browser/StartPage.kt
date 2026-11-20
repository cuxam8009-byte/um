package dev.dymith.browser

/**
 * Trang chủ nằm ở assets/start.html và được phục vụ qua shouldInterceptRequest tại một
 * origin https ảo (appassets.androidplatform.net là tên miền mà WebViewAssetLoader của AndroidX
 * cũng dùng cho mục đích này; mọi request tới đó đều bị chặn lại và trả từ assets, không ra mạng). Nhờ vậy lịch sử
 * Back/Forward, localStorage và cầu nối JS giới hạn theo origin đều hoạt động đúng.
 */
object StartPage {
    const val HOST = "appassets.androidplatform.net"
    const val ORIGIN = "https://appassets.androidplatform.net"
    const val URL = "https://appassets.androidplatform.net/"

    fun isStartUrl(url: String?): Boolean = url != null && url.startsWith(URL)
}
