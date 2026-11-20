# Dymith Browser

Trình duyệt Android nhẹ, lõi viết bằng Rust, giao diện kính kiểu Apple.
Repo đang ở **Phase 0.2**: giao diện đen-trắng mới, lõi Rust có bộ chặn quảng cáo và lịch sử.

## Cấu trúc

| Thư mục | Nội dung |
|---|---|
| `core/` | Lõi Rust: ô địa chỉ, bộ chặn quảng cáo (luật kiểu ABP), lịch sử + gợi ý, cầu JNI |
| `core-lite/` | Lõi v0.0.1 dự phòng: CI chỉ dùng khi `core/` không biên dịch được |
| `app/` | Vỏ Android (Kotlin, View thuần): WebView + trang chủ HTML (`assets/start.html`) + thanh công cụ phẳng |
| `ci/build.sh` | Toàn bộ quy trình build: Rust, kiểm tra căn 16 KB, Gradle, Release |
| `docs/PHASE0.md` | Danh sách bài thử cho Phase 0 |

## Cập nhật mã

Upload một file `.zip` vào thư mục gốc của repo. Workflow `Dymith build` sẽ tự giải nén,
commit, dựng APK và đăng lên mục Releases (tag `nightly`).

## Kiến trúc

Lõi Rust nói chuyện với vỏ Kotlin qua JNI. Engine web đứng sau một giao diện trừu tượng:
v1 dùng WebView hệ thống, Servo và GeckoView là các hướng thử nghiệm ở Phase 3.
