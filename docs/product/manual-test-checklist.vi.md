# Checklist kiểm thử thủ công RustlingPDF

Tài liệu này dùng để tự tay kiểm thử toàn bộ tính năng của RustlingPDF trước khi
phát hành. Mọi mục đều được suy ra từ code thực tế: registry công cụ frontend
(`frontend/editor/src/core/data/useTranslatedToolRegistry.tsx`), router Axum
(`rust/crates/rustling-processing/src/lib.rs`) và bảng endpoint trong
[`features.md`](features.md).

Quy ước:

- `[ ]` chưa test · `[x]` pass · `[!]` fail (ghi số issue hoặc mô tả ở cột ghi chú)
- ⚙️ = endpoint chỉ hoạt động khi có external tool tương ứng. Nếu máy không có
  tool đó, kết quả **đúng** là báo lỗi rõ ràng (thường HTTP `501`), không phải
  fallback im lặng.
- 🤖 = chỉ chạy khi bật AI engine (mặc định tắt).

---

## 0. Chuẩn bị

### 0.1 File mẫu

Thư mục [`testing/`](../../testing) có sẵn: `test_pdf_1.pdf` … `test_pdf_4.pdf`,
`crop_test.pdf`. Ngoài ra tự chuẩn bị thêm:

- [ ] PDF nhiều trang (≥ 20 trang) có bookmark/outline
- [ ] PDF scan (ảnh, không có text layer) để test OCR
- [ ] PDF có form fields (AcroForm)
- [ ] PDF đã đặt mật khẩu (biết password)
- [ ] PDF đã ký số (digital signature)
- [ ] PDF có attachment nhúng + có JavaScript nhúng
- [ ] PDF hỏng/malformed để test Repair
- [ ] File Office: `.docx`, `.xlsx`, `.pptx`
- [ ] Ảnh: `.jpg`, `.png`; file `.svg`, `.html`, `.md`, `.eml`, `.epub`, `.cbz`
- [ ] File CSV/XLSX chứa dữ liệu để test Batch form fill

### 0.2 Khởi động môi trường

Chọn ít nhất một, lý tưởng là test cả ba:

- [ ] **Từ source**: `task dev` (backend + web frontend) — mở URL frontend in ra
- [ ] **Docker**: `docker compose -f docker/compose.yml up` — kiểm tra image chạy được
- [ ] **Desktop**: chạy app Tauri đã build (mục [10](#10-ứng-dụng-desktop-tauri))
- [ ] (tuỳ chọn) `task dev:all` để bật thêm AI engine

### 0.3 Kiểm tra sức khoẻ hệ thống

- [ ] `GET /api/v1/info/status` trả về OK
- [ ] `GET /api/v1/config/endpoints-availability` trả về danh sách; các endpoint
      thiếu dependency báo `{"enabled": false, "reason": "DEPENDENCY"}`
- [ ] Đối chiếu: tool nào bị disable trong UI đúng bằng danh sách trên
- [ ] Backend không crash khi thiếu external tool (chỉ endpoint đó không dùng được)

---

## 1. Vỏ ứng dụng & workspace

- [ ] Trang chủ hiển thị đủ 4 nhóm: Recommended / Standard / Advanced tools
- [ ] Tìm kiếm công cụ theo tên (cả tiếng Anh và tiếng đã dịch) ra kết quả đúng
- [ ] Đánh dấu favorite một công cụ → hiện ở nhóm favorite, giữ sau khi reload
- [ ] Gợi ý công cụ (suggested tools) xuất hiện hợp lý sau khi dùng một tool
- [ ] URL đồng bộ với công cụ đang mở (copy URL → mở tab mới vào đúng tool)
- [ ] Chuyển đổi light/dark theme, màu và độ tương phản đúng ở cả hai chế độ
- [ ] Đổi ngôn ngữ (có 40 locale, gồm `vi-VN`) → UI dịch, không lộ key thô
- [ ] RTL: chọn `ar-AR` hoặc `fa-IR`, layout không vỡ
- [ ] Responsive: thu nhỏ cửa sổ / mobile viewport, sidebar và toolbar vẫn dùng được

### 1.1 File manager

- [ ] Upload bằng nút chọn file
- [ ] Upload bằng drag & drop
- [ ] Upload nhiều file cùng lúc
- [ ] Thumbnail/preview hiển thị đúng trang đầu
- [ ] Đổi tên file trong workspace
- [ ] Xoá một file, xoá tất cả
- [ ] Chọn nhiều file cho tool đa-input (Merge, Compare, Overlay)
- [ ] Download kết quả: một file và nhiều file (ZIP)
- [ ] Kết quả của tool này dùng được làm input cho tool tiếp theo (chaining)
- [ ] Reload trang → trạng thái file xử lý đúng như thiết kế, không rò rỉ blob

### 1.2 Viewer / Read

- [ ] Mở PDF, lật trang, nhảy tới số trang
- [ ] Zoom in/out, fit width/page
- [ ] Xem sidebar thumbnail, click chuyển trang
- [ ] Xem bookmark/outline, click nhảy đúng vị trí
- [ ] Tìm text trong tài liệu, highlight kết quả
- [ ] In tài liệu (print)
- [ ] Mở PDF lớn (≥ 50 MB / ≥ 300 trang) không treo UI

---

## 2. Recommended tools

| Công cụ | Test | Ghi chú |
|---|---|---|
| PDF Text Editor | [ ] Sửa text tại chỗ, lưu, mở lại thấy nội dung mới | |
| PDF Text Editor | [ ] Font/khoảng cách không bị phá vỡ sau khi lưu | |
| Multi-Tool | [ ] Xoay, xoá, sắp xếp lại, chèn trang trong một phiên | |
| Multi-Tool | [ ] Undo/redo hoạt động đúng | |
| Merge | [ ] Ghép 3 PDF, đúng thứ tự, đủ số trang | |
| Merge | [ ] Đổi thứ tự file trước khi ghép, kết quả theo thứ tự mới | |
| Compare | [ ] So sánh 2 PDF khác nhau → chỉ ra được điểm khác | |
| Compare | [ ] So sánh 2 PDF giống nhau → báo không khác biệt | |
| Compress | [ ] Nén giảm dung lượng, nội dung còn đọc được | |
| Compress | [ ] Thử các mức nén khác nhau, so sánh size/chất lượng | |
| Convert | [ ] Xem mục [3](#3-convert) | |
| OCR ⚙️ | [ ] OCR PDF scan → text select/search được | Cần Tesseract hoặc OCRmyPDF |
| OCR ⚙️ | [ ] Ngôn ngữ OCR khác (nếu có tessdata) | Desktop build có sẵn tiếng Anh |
| Redact | [ ] Redact thủ công theo vùng chọn | |
| Redact | [ ] Auto-redact theo từ khoá/regex | |
| Redact | [ ] Sau khi "burn in", copy text vùng đã redact **không** ra nội dung cũ | Quan trọng |

---

## 3. Convert

### 3.1 Sang PDF

- [ ] `.docx` → PDF (built-in, không cần LibreOffice)
- [ ] `.xlsx` → PDF (built-in)
- [ ] `.pptx` → PDF (built-in)
- [ ] Định dạng Office khác (`.odt`, `.doc`…) → PDF ⚙️ LibreOffice
- [ ] Ảnh (JPEG/PNG) → PDF, nhiều ảnh thành nhiều trang
- [ ] `.svg` → PDF
- [ ] `.html` → PDF ⚙️ WeasyPrint
- [ ] `.md` → PDF ⚙️ WeasyPrint
- [ ] `.eml` → PDF ⚙️ WeasyPrint
- [ ] URL → PDF ⚙️ WeasyPrint **và** phải bật `RUSTLING_PROCESSING_ENABLE_URL_TO_PDF=true`
- [ ] URL → PDF khi chưa bật: bị từ chối (SSRF guard hoạt động)
- [ ] Thử URL nội bộ (`127.0.0.1`, `169.254.169.254`) → bị chặn
- [ ] `.cbz` → PDF
- [ ] `.cbr` → PDF ⚙️ unrar/7z
- [ ] Ebook (`.epub`/`.mobi`) → PDF ⚙️ Calibre
- [ ] Auto-detect định dạng đầu vào chọn đúng endpoint

### 3.2 Từ PDF

- [ ] PDF → ảnh (kiểm tra chọn định dạng, DPI, một/nhiều trang)
- [ ] PDF → text (`txt`) và → `rtf`
- [ ] PDF → Markdown
- [ ] PDF → HTML
- [ ] PDF → CSV (PDF có bảng)
- [ ] PDF → XLSX (PDF có bảng)
- [ ] PDF → `.cbz`
- [ ] PDF → `.cbr` ⚙️ cần `rar`
- [ ] PDF → Word ⚙️ LibreOffice
- [ ] PDF → PowerPoint ⚙️ LibreOffice
- [ ] PDF → XML ⚙️ LibreOffice
- [ ] PDF → EPUB ⚙️ Calibre
- [ ] PDF → video (MP4/WebM) ⚙️ FFmpeg
- [ ] Convert nhiều file cùng lúc: mỗi input ra một output riêng

---

## 4. Standard tools

### 4.1 Signing

- [ ] **Certificate Sign**: ký bằng file `.p12`/`.pfx` + mật khẩu → signature xuất hiện
- [ ] Certificate Sign: sai mật khẩu → báo lỗi rõ ràng
- [ ] Certificate Sign: chọn vị trí/trang hiển thị chữ ký
- [ ] Certificate Sign qua hardware token (PKCS#11) nếu có thiết bị
- [ ] Trên Windows: liệt kê được certificate trong Windows certificate store
- [ ] **Timestamp PDF**: áp timestamp RFC 3161 với TSA URL hợp lệ
- [ ] Timestamp PDF: TSA URL sai/không phản hồi → lỗi rõ ràng, không treo
- [ ] **Sign**: vẽ chữ ký tay, upload ảnh chữ ký, gõ text
- [ ] Sign: kéo/đổi kích thước chữ ký, chọn trang, lưu ra PDF đúng vị trí

### 4.2 Document security

- [ ] **Add Password**: đặt user password → mở file cần password
- [ ] Add Password: đặt owner password + giới hạn quyền
- [ ] **Change Permissions**: chặn in/copy/sửa → kiểm tra trong reader
- [ ] **Add Watermark**: watermark text (đổi font, size, góc, độ mờ, lặp)
- [ ] Add Watermark: watermark bằng ảnh
- [ ] **Add Stamp**: stamp text và stamp ảnh, chọn trang và vị trí
- [ ] **Sanitize**: xoá JavaScript → Show JS không còn thấy script
- [ ] Sanitize: xoá embedded files / metadata / links (test từng tuỳ chọn)
- [ ] **Flatten**: flatten form fields → không còn field điền được
- [ ] Flatten: flatten annotation vào nội dung trang
- [ ] **Unlock PDF Forms**: form bị read-only → sau khi unlock điền được

### 4.3 Verification / Inspect

- [ ] **Get ALL Info on PDF**: đủ metadata, permission, form field, embedded content
- [ ] Get Info trên PDF mã hoá và PDF đã ký → thông tin phản ánh đúng
- [ ] **Validate PDF Signature**: file ký hợp lệ → valid
- [ ] Validate Signature: file bị sửa sau khi ký → báo không hợp lệ
- [ ] **Accessibility check**: báo cáo tagged structure, language, reading order, alt text
- [ ] Accessibility remediate: đặt language, tab order, alt text cho Figure, label form
- [ ] Accessibility: báo cáo **không** tự nhận là PDF/UA certified
- [ ] Verify PDF: file không khai báo profile → trả `not-pdfa`, không lỗi
- [ ] Verify PDF: file khai báo PDF/A mà máy không có veraPDF → từ chối rõ ràng (`501`)

### 4.4 Document review

- [ ] **Change Metadata**: sửa title/author/subject/keywords/creator/producer/dates
- [ ] Change Metadata: xoá sạch metadata
- [ ] **Edit Table of Contents**: đọc outline hiện có, sửa cây bookmark, lưu, mở lại đúng
- [ ] Edit TOC: tạo bookmark nhiều cấp

### 4.5 Page formatting

- [ ] **Crop**: cắt bằng kéo vùng chọn (dùng `crop_test.pdf`)
- [ ] Crop: áp cho một trang / tất cả trang
- [ ] **Rotate**: xoay 90/180/270, một trang và toàn bộ
- [ ] **Split**: theo số trang cụ thể
- [ ] Split: theo dung lượng hoặc số trang mỗi phần
- [ ] Split: theo chapter/bookmark
- [ ] Split: chia mỗi trang thành lưới (sections)
- [ ] Split: poster print (chia một trang lớn thành nhiều tờ)
- [ ] **Reorganize Pages**: nhập thứ tự thủ công
- [ ] Reorganize Pages: dùng preset (reverse, odd/even, duplex…)
- [ ] **Adjust page size/scale**: đổi sang A4/Letter, kiểm tra tỉ lệ nội dung
- [ ] **Add Page Numbers**: chọn vị trí, số bắt đầu, khoảng trang, định dạng
- [ ] **Multi-Page Layout** (N-up): 2/4/9 trang trên một trang
- [ ] **Booklet Imposition**: thứ tự trang đúng để in gấp thành sổ
- [ ] **PDF to Single Large Page**: nối mọi trang thành một trang dài
- [ ] **Add Attachments**: đính kèm file, list, rename, xoá attachment
- [ ] Extract attachments ra ZIP

### 4.6 Extraction

- [ ] **Extract Pages**: theo danh sách/khoảng trang
- [ ] **Extract Images**: lấy ảnh nhúng, kiểm tra số lượng và chất lượng
- [ ] Extract Images: PDF không có ảnh → thông báo hợp lý, không lỗi tối nghĩa

### 4.7 Removal

- [ ] **Remove Pages**: xoá trang chỉ định
- [ ] **Remove Blank Pages**: PDF có trang trắng → bị loại, ngưỡng nhận diện hợp lý
- [ ] **Remove Annotations**: xoá hết annotation
- [ ] **Remove Images**: xoá ảnh, text còn nguyên
- [ ] **Remove Password**: đúng password → mở được không cần password
- [ ] Remove Password: sai password → báo lỗi, không tạo file rác
- [ ] **Remove Certificate Sign**: xoá signature khỏi file đã ký

### 4.8 Forms (4 chế độ: Fill / Create / Batch / Modify)

- [ ] **Fill**: liệt kê field, điền text/checkbox/radio/dropdown, lưu, mở lại còn giá trị
- [ ] **Create**: đặt và resize widget trực tiếp trên trang, field mới điền được
- [ ] **Modify**: đổi thuộc tính field (tên, required, read-only…)
- [ ] Xoá field
- [ ] **Batch**: upload CSV → mỗi dòng ra một PDF, tải về ZIP
- [ ] Batch: upload XLSX → tương tự
- [ ] Batch: CSV thiếu cột / sai header → lỗi rõ ràng
- [ ] Export giá trị form ra CSV và XLSX

---

## 5. Advanced tools

### 5.1 Automation

- [ ] **Automate**: dựng pipeline nhiều bước (ví dụ Merge → Compress → Add Watermark)
- [ ] Automate: chạy pipeline, tải kết quả cuối
- [ ] Automate: lưu / tải lại / sửa pipeline đã lưu
- [ ] Automate: có bước lỗi → báo lỗi ở đúng bước, không im lặng bỏ qua
- [ ] Automate với filter điều kiện: contains-text, contains-image, file-size,
      page-count, page-rotation, page-size (mỗi filter test cả nhánh pass và fail)
- [ ] **Auto Rename**: file được đổi tên theo title phát hiện được
- [ ] Auto Rename: PDF không có title rõ → hành vi fallback hợp lý
- [ ] Async job: theo dõi trạng thái job, tải kết quả, huỷ job đang chạy
- [ ] `GET /api/v1/jobs/stats` và `queue/stats` phản ánh đúng số job

### 5.2 Advanced formatting

- [ ] **Adjust Colors/Contrast**: đổi brightness/contrast/saturation
- [ ] **Repair** ⚙️: PDF hỏng → mở được sau khi repair (desktop build có qpdf sẵn)
- [ ] **Detect & Split Scanned Photos**: ảnh scan nhiều tấm → tách thành từng tấm
- [ ] **Overlay PDFs**: chồng PDF lên PDF, thử các chế độ overlay
- [ ] **Replace & Invert Color**: invert toàn bộ (dark mode PDF), replace màu cụ thể
- [ ] **Scanner Effect**: PDF trông như bản scan (thử các mức độ)
- [ ] Auto-split scanned batch theo trang phân cách/QR
- [ ] Decompress PDF để xem stream bên trong

### 5.3 Developer tools

- [ ] **Show JavaScript**: PDF có JS → hiển thị đúng script
- [ ] Show JavaScript: PDF không có JS → thông báo rõ
- [ ] **API (Swagger UI)**: mở được, "Try it out" gọi endpoint thật thành công
- [ ] Swagger snapshot khớp với endpoint thực tế (không có route lạ/thiếu)
- [ ] **Automated Folder Scanning**: theo tài liệu hướng dẫn, thả file vào folder → được xử lý
- [ ] **Air-gapped Setup**: làm theo hướng dẫn, app chạy không cần internet

---

## 6. Mobile scanner

- [ ] Mở `/mobile-scanner` trên điện thoại (cùng mạng LAN)
- [ ] Chụp bằng camera; chọn nhiều ảnh từ thư viện
- [ ] Tự động phát hiện viền tài liệu
- [ ] Kéo 4 góc để chỉnh phối cảnh
- [ ] Xoay ảnh, áp filter làm sạch tài liệu
- [ ] Sắp xếp lại thứ tự trang
- [ ] Xuất ra một PDF nhiều trang ngay trên máy (không cần session)
- [ ] Luồng QR từ desktop: quét QR → phiên mở → gửi file lên → desktop nhận được
- [ ] Sau khi tải về, file bị xoá khỏi session (tải lần hai không còn)
- [ ] Kết thúc session (DELETE) → session không còn hợp lệ
- [ ] Session hết hạn sau ~10 phút
- [ ] Offline: sau lần load đầu, mở lại scanner khi mất mạng vẫn chạy phần local

---

## 7. CLI (`rustlingpdf`)

- [ ] `rustlingpdf operations` liệt kê operation; `--json` ra JSON hợp lệ
- [ ] `rustlingpdf describe <operation>` in schema tham số
- [ ] `describe` nhận cả operation ID và đường dẫn `/api/v1/...`
- [ ] `rustlingpdf run <op> -i in.pdf -o out.pdf -p key=value` chạy đúng
- [ ] `run` với `--params-json` inline và `@file.json`
- [ ] `run` với nhiều `-i` cho operation đa input
- [ ] `run -o -` xuất binary ra stdout (pipe được)
- [ ] `run` khi output đã tồn tại → từ chối; thêm `--force` thì ghi đè
- [ ] `rustlingpdf pipeline --spec pipeline.json -i in.pdf -o out.pdf`
- [ ] Tham số sai kiểu/thiếu → exit code `2` (usage) hoặc `4` (rejected), message rõ
- [ ] Thiếu external tool → exit code `5` (unavailable)
- [ ] File không đọc được → exit code `3` (io)
- [ ] CLI chạy được **không** cần khởi động HTTP server

---

## 8. AI engine (tuỳ chọn) 🤖

Chỉ test khi đã bật engine (`AIENGINE_ENABLED=true`, có `AIENGINE_URL` và API key
của bạn, hoặc Ollama self-hosted).

- [ ] Khi engine **tắt**: các tool AI ẩn/disable, `ai/health` báo unreachable,
      backend vẫn chạy bình thường
- [ ] Khi engine tắt: xác nhận không có request nào ra ngoài (theo dõi network)
- [ ] `GET /api/v1/ai/health` → reachable khi engine bật
- [ ] Document summary: có trích dẫn số trang, và số trang đó đúng
- [ ] Document extraction: khai báo schema riêng → trả đúng field + trang nguồn
- [ ] Document translation: giữ được ranh giới trang và thứ tự block
- [ ] Classify and label: phân loại theo tập nhãn tự cung cấp
- [ ] Math auditor: kiểm tra phép tính/số liệu trong PDF
- [ ] PDF comment agent: sinh sticky-note comment vào PDF
- [ ] AI PDF edit: sinh plan rồi thực thi, kết quả đúng ý định
- [ ] Create PDF from description: sinh PDF từ mô tả có cấu trúc
- [ ] Orchestrate và `orchestrate/stream` (feed NDJSON tiến độ)
- [ ] Vượt giới hạn trang/ký tự cấu hình → bị chặn có thông báo
- [ ] Không tồn tại chức năng PDF question-answering (đã bị loại bỏ có chủ ý)

---

## 9. Bảo mật & trường hợp biên

- [ ] Upload file **không phải PDF** vào tool chỉ nhận PDF → từ chối rõ ràng
- [ ] Upload file rỗng (0 byte) → lỗi rõ ràng, không crash
- [ ] Upload file quá lớn so với giới hạn → bị chặn đúng thông báo
- [ ] PDF mã hoá vào tool cần đọc nội dung → yêu cầu password hoặc lỗi rõ ràng
- [ ] Tên file có ký tự đặc biệt/unicode/dấu tiếng Việt → xử lý và tải về đúng tên
- [ ] Tên file kiểu path traversal (`../../x.pdf`) → bị làm sạch
- [ ] Số trang ngoài phạm vi (ví dụ trang 999 của PDF 5 trang) → lỗi rõ ràng
- [ ] Sau khi xử lý, thư mục temp được dọn (không tích tụ file)
- [ ] Không có endpoint nào yêu cầu login (đúng thiết kế: không có auth)
- [ ] Không có request analytics/telemetry nào phát ra
- [ ] Counter `/api/v1/info/*` reset sau khi restart (chỉ in-memory)
- [ ] Gửi song song nhiều request nặng → server không sập, có backpressure

---

## 10. Ứng dụng desktop (Tauri)

### 10.1 Cài đặt

Test trên các nền tảng bạn phát hành (`deb`, `rpm`, `appimage`, `dmg`, `app`,
`msi`, `nsis`):

- [ ] Windows MSI: cài, mở, gỡ cài đặt sạch
- [ ] Windows MSI: tuỳ chọn tạo shortcut desktop lúc cài — chọn **có** → có shortcut
- [ ] Windows MSI: chọn **không** → không tạo shortcut
- [ ] Windows NSIS installer
- [ ] macOS `.dmg`/`.app`: mở được, không bị Gatekeeper chặn ngoài dự kiến
- [ ] Linux `.deb` và `.rpm`: cài, có entry trong menu ứng dụng
- [ ] AppImage (nếu đã bật lại leg này)
- [ ] Ghi lại dung lượng file cài **đã nén** để so với bản trước

### 10.2 Hành vi runtime

- [ ] Mở app lần đầu: sidecar backend tự khởi động, UI kết nối thành công
- [ ] Đóng app: process sidecar được kết thúc (không còn tiến trình treo)
- [ ] Mở lại app nhiều lần liên tiếp không bị lỗi cổng đang dùng
- [ ] File association: double-click file `.pdf` → mở trong RustlingPDF
- [ ] Đặt app làm ứng dụng mặc định cho PDF (nếu có nút trong app)
- [ ] Kéo file từ Explorer/Finder vào cửa sổ app
- [ ] Preview tài liệu Word (`.docx`) trong app
- [ ] Print từ app ra máy in thật hoặc "Print to PDF"
- [ ] Cửa sổ: minimize/maximize/resize, trạng thái được ghi nhớ
- [ ] Log file được ghi ở đúng thư mục, hữu ích khi debug
- [ ] Chạy hoàn toàn offline (ngắt mạng) → mọi tool local vẫn hoạt động
- [ ] qpdf bundled: Repair chạy được mà không cài thêm gì
- [ ] Tesseract bundled: OCR tiếng Anh chạy được mà không cài thêm gì

---

## 11. Docker

- [ ] `docker compose -f docker/compose.yml up` khởi động thành công
- [ ] Truy cập được UI và API từ host
- [ ] Các external tool có trong image hoạt động (LibreOffice/WeasyPrint/… tuỳ image)
- [ ] veraPDF **không** có trong image → verify PDF/A bị từ chối rõ ràng (đúng kỳ vọng)
- [ ] Profile `ai`: `docker compose --profile ai up` mới bật AI engine
- [ ] Không có profile `ai` → container AI engine không chạy
- [ ] Container restart → hoạt động bình thường, không còn state tài liệu cũ
- [ ] Biến môi trường `RUSTLING_*` truyền vào container có tác dụng

---

## 12. Cổng chất lượng tự động (chạy trước khi kết luận)

- [ ] `task rust:check` pass
- [ ] `task frontend:check` pass
- [ ] `task engine:check` pass
- [ ] `task desktop:test` pass
- [ ] `task check:all` pass

> Trước khi build/test Rust, kiểm tra `rust/target/debug/deps`; nếu vượt 50 GB
> thì chạy `task rust:clean:deps`.

---

## 13. Mẫu ghi kết quả

| Ngày | Môi trường | Phiên bản | Mục fail | Ghi chú |
|---|---|---|---|---|
| | web / docker / desktop | | | |

Với mỗi mục `[!]`, ghi lại: file input dùng gì, tham số nào, hành vi mong đợi vs
hành vi thực tế, và log/response lỗi.
