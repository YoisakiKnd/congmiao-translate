use tauri::AppHandle;

pub fn begin(app: &AppHandle) {
    let silent = crate::capture::take_silent();
    let app = app.clone();
    std::thread::spawn(move || {
        crate::popup::open_at_cursor(&app, "按住鼠标拖拽要识别的区域，Esc 取消", "notice");
        match capture_and_recognize(&app) {
            Ok(text) if text.trim().is_empty() => {
                crate::popup::open_at_cursor(&app, "没有识别到文字", "notice");
            }
            Ok(text) if silent => {
                crate::clipboard::write_text(text.trim());
                crate::popup::open_at_cursor(&app, "已复制识别到的文字", "notice");
            }
            Ok(text) => crate::popup::open_at_cursor(&app, text.trim(), "ocr"),
            Err(message) => crate::popup::open_at_cursor(&app, &message, "notice"),
        }
    });
}

fn capture_and_recognize(app: &AppHandle) -> Result<String, String> {
    let png = capture_region(app)?;
    recognize_png(&png)
}

fn capture_region(app: &AppHandle) -> Result<Vec<u8>, String> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        HGDIOBJ, SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let rect = drag_rect(app)?;
    unsafe {
        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);
        let rect = RECT {
            left: rect.left.clamp(0, width.saturating_sub(1)),
            top: rect.top.clamp(0, height.saturating_sub(1)),
            right: rect.right.clamp(0, width),
            bottom: rect.bottom.clamp(0, height),
        };
        let w = rect.right - rect.left;
        let h = rect.bottom - rect.top;
        if w < 8 || h < 8 {
            return Err("框选区域太小".into());
        }
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, w, h);
        let previous = SelectObject(memory, HGDIOBJ::from(bitmap));
        BitBlt(
            memory,
            0,
            0,
            w,
            h,
            Some(screen),
            rect.left,
            rect.top,
            SRCCOPY,
        )
        .map_err(|err| err.to_string())?;
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        GetDIBits(
            memory,
            bitmap,
            0,
            h as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(memory, previous);
        let _ = DeleteObject(HGDIOBJ::from(bitmap));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        Ok(bmp_bytes(w, h, &pixels))
    }
}

fn drag_rect(app: &AppHandle) -> Result<windows::Win32::Foundation::RECT, String> {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let started = std::time::Instant::now();
    while unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0 {
        if started.elapsed() > std::time::Duration::from_secs(2) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    let started = std::time::Instant::now();
    let mut origin = POINT::default();
    let mut dragging = false;
    loop {
        if started.elapsed() > std::time::Duration::from_secs(60) {
            return Err("没有在时限内框选区域".into());
        }
        if unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) } < 0 {
            return Err("已取消".into());
        }
        let pressed = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
        if pressed && !dragging {
            unsafe { GetCursorPos(&mut origin) }.map_err(|err| err.to_string())?;
            crate::popup::hide(app);
            dragging = true;
        } else if dragging && !pressed {
            let mut end = POINT::default();
            unsafe { GetCursorPos(&mut end) }.map_err(|err| err.to_string())?;
            let Some((left, top, width, height)) =
                super::drag_bounds(origin.x, origin.y, end.x, end.y)
            else {
                return Err("框选区域太小".into());
            };
            return Ok(RECT {
                left,
                top,
                right: left + width,
                bottom: top + height,
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

fn bmp_bytes(width: i32, height: i32, pixels: &[u8]) -> Vec<u8> {
    let row = (width as usize) * 4;
    let pixel_size = row * height as usize;
    let offset = 54u32;
    let mut bytes = vec![0u8; offset as usize + pixel_size];
    bytes[0] = b'B';
    bytes[1] = b'M';
    let file_size = bytes.len() as u32;
    bytes[2..6].copy_from_slice(&file_size.to_le_bytes());
    bytes[10..14].copy_from_slice(&offset.to_le_bytes());
    bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
    bytes[18..22].copy_from_slice(&width.to_le_bytes());
    bytes[22..26].copy_from_slice(&height.to_le_bytes());
    bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&32u16.to_le_bytes());
    bytes[offset as usize..].copy_from_slice(&pixels[..pixel_size]);
    bytes
}

pub(crate) fn recognize_png(bytes: &[u8]) -> Result<String, String> {
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    let stream = InMemoryRandomAccessStream::new().map_err(|err| err.to_string())?;
    let writer = DataWriter::CreateDataWriter(&stream).map_err(|err| err.to_string())?;
    writer.WriteBytes(bytes).map_err(|err| err.to_string())?;
    writer
        .StoreAsync()
        .map_err(|err| err.to_string())?
        .get()
        .map_err(|err| err.to_string())?;
    writer.DetachStream().map_err(|err| err.to_string())?;
    stream.Seek(0).map_err(|err| err.to_string())?;
    let decoder = BitmapDecoder::CreateAsync(&stream)
        .map_err(|err| err.to_string())?
        .get()
        .map_err(|err| err.to_string())?;
    let bitmap = decoder
        .GetSoftwareBitmapAsync()
        .map_err(|err| err.to_string())?
        .get()
        .map_err(|err| err.to_string())?;
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|err| err.to_string())?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(|err| err.to_string())?
        .get()
        .map_err(|err| err.to_string())?;
    result
        .Text()
        .map(|text| text.to_string())
        .map_err(|err| err.to_string())
}
