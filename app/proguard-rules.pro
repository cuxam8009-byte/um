-keepattributes SourceFile,LineNumberTable
-keep class dev.dymith.browser.Core { *; }
# androidx.webkit nói chuyện với WebView hệ thống qua phản chiếu: giữ nguyên để cầu nối trang chủ hoạt động.
-keep class androidx.webkit.** { *; }
-keep class org.chromium.support_lib_boundary.** { *; }
-dontwarn org.chromium.support_lib_boundary.**
