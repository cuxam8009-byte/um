# Bài thử Phase 0.2

Sau khi thử, vào **Menu (3 chấm) > Chẩn đoán và báo cáo > Sao chép** rồi dán cho Claude.
Menu chỉ hiện khi đang xem một website (trang chủ có thanh tìm kiếm riêng).

## 1. Giao diện mới và độ mượt
- Mở app: trang chủ đen-trắng, đồng hồ, lối tắt, thanh tìm kiếm ở đáy.
- Cuộn trang chủ, mở bàn phím ở ô tìm kiếm, đổi công cụ tìm kiếm, đổi "Giao diện" (Tự động/Sáng/Tối).
  Thanh trạng thái và thanh công cụ native phải đổi màu theo.
- Mở một trang dài (Wikipedia), cuộn 30 giây: thanh công cụ phải trượt xuống khi cuộn xuống.
- Chẩn đoán > dòng "Độ mượt" (so với bản trước nếu bạn còn số cũ).

## 2. Lõi Rust v0.0.2
- Chẩn đoán > "Lõi Rust" phải là `dymith-core 0.0.2`. Nếu là 0.0.1 nghĩa là lõi mới lỗi biên dịch
  và CI đã dùng lõi dự phòng (xem báo cáo bản dựng, có kèm lỗi).
- "Tự kiểm tra bộ lọc" phải là `ok`.
- Menu > "Cập nhật bộ lọc quảng cáo" (cần mạng), rồi mở VnExpress và trang thử quảng cáo
  https://d3ward.github.io/toolz/adblock.html. Xem "Thống kê lõi" (số luật, số lượt chặn).
- Gợi ý lịch sử: duyệt vài trang, về trang chủ, gõ "ban hang" hoặc một phần tên trang đã xem
  (có/không dấu): phải hiện dòng gợi ý từ lịch sử.
- Menu > "Xóa lịch sử duyệt web".

## 3. Vẫn cần thử như bản trước
- Gõ tiếng Việt (Telex/VNI) ở ô địa chỉ native và ô tìm kiếm trên trang chủ.
- Đăng nhập Google (accounts.google.com), thử bật "UA chuẩn trình duyệt".

## Chưa có
Nhiều tab, tải xuống, tải tệp lên, video toàn màn hình, dấu trang, tìm trong trang.
