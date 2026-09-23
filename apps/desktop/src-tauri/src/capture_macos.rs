use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::OnceLock;

use block2::RcBlock;
use objc2::runtime::ClassBuilder;
use tauri::AppHandle;

use crate::objc_compat::{
    class, id, msg_send, nil, ns_string as compat_ns_string, sel, Class, NSPoint, NSRect, NSSize,
    Object, Sel, BOOL, NO, YES,
};

#[link(name = "Vision", kind = "framework")]
extern "C" {}

const ESCAPE_KEY: u16 = 53;
const SCREEN_SAVER_LEVEL: i64 = 1000;
const FLOATING_LEVEL: i64 = 3;
const JOIN_ALL_SPACES: usize = 1 | (1 << 8);
const COMPOSITE_COPY: usize = 1;
const NONACTIVATING_PANEL: usize = 1 << 7;
const KEY_DOWN_MASK: usize = 1 << 10;

struct Session {
    anchor: Option<NSPoint>,
    current: Option<NSPoint>,
    window: id,
    view: id,
    app: AppHandle,
    local_monitor: id,
    global_monitor: id,
    local_block: RcBlock<dyn Fn(id) -> id>,
    global_block: RcBlock<dyn Fn(id)>,
}

struct Panel {
    window: id,
    body: id,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static PANEL: RefCell<Option<Panel>> = const { RefCell::new(None) };
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCreateImage(
        screen_bounds: CGRect,
        list_option: u32,
        window_id: u32,
        image_option: u32,
    ) -> *mut c_void;
    fn CGImageRelease(image: *mut c_void);
    fn CGPreflightScreenCaptureAccess() -> u8;
    fn CGRequestScreenCaptureAccess() -> u8;
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    fn NSRectFill(rect: NSRect);
    fn NSRectFillUsingOperation(rect: NSRect, op: usize);
}

#[link(name = "Vision", kind = "framework")]
extern "C" {}

pub fn begin(app: &AppHandle) {
    if SESSION.with(|session| session.borrow().is_some()) {
        return;
    }
    let frame = union_screen_frame();
    if frame.size.width < 1.0 || frame.size.height < 1.0 {
        return;
    }
    let window: id = unsafe { msg_send![panel_class(), alloc] };
    let window: id = unsafe {
        msg_send![window, initWithContentRect: frame,
            styleMask: NONACTIVATING_PANEL,
            backing: 2usize,
            defer: NO]
    };
    let view_frame = NSRect::new(NSPoint::new(0.0, 0.0), frame.size);
    let view: id = unsafe { msg_send![view_class(), alloc] };
    let view: id = unsafe { msg_send![view, initWithFrame: view_frame] };
    let local_block = RcBlock::new(|event: id| -> id {
        if is_escape(event) {
            cancel();
            nil
        } else {
            event
        }
    });
    let global_block = RcBlock::new(|event: id| {
        if is_escape(event) {
            cancel();
        }
    });
    let (local_monitor, global_monitor) = unsafe {
        let clear: id = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![window, setOpaque: NO];
        let _: () = msg_send![window, setBackgroundColor: clear];
        let _: () = msg_send![window, setLevel: SCREEN_SAVER_LEVEL];
        let _: () = msg_send![window, setCollectionBehavior: JOIN_ALL_SPACES];
        let _: () = msg_send![window, setIgnoresMouseEvents: NO];
        let _: () = msg_send![window, setHasShadow: NO];
        let _: () = msg_send![window, setHidesOnDeactivate: NO];
        let _: () = msg_send![window, setBecomesKeyOnlyIfNeeded: NO];
        let _: () = msg_send![window, setReleasedWhenClosed: NO];
        let _: () = msg_send![window, setContentView: view];
        let _: () = msg_send![window, makeFirstResponder: view];
        let _: () = msg_send![window, orderFrontRegardless];
        let _: () = msg_send![window, makeKeyWindow];
        let local: id = msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask: KEY_DOWN_MASK, handler: &*local_block];
        let global: id = msg_send![class!(NSEvent), addGlobalMonitorForEventsMatchingMask: KEY_DOWN_MASK, handler: &*global_block];
        (local, global)
    };
    SESSION.with(|slot| {
        *slot.borrow_mut() = Some(Session {
            anchor: None,
            current: None,
            window,
            view,
            app: app.clone(),
            local_monitor,
            global_monitor,
            local_block,
            global_block,
        });
    });
}

fn panel_class() -> &'static Class {
    static CLASS: OnceLock<&'static Class> = OnceLock::new();
    CLASS.get_or_init(|| {
        let mut decl = ClassBuilder::new(c"CongmiaoCapturePanel", class!(NSPanel))
            .expect("无法注册截图遮罩窗口");
        unsafe {
            decl.add_method(
                sel!(canBecomeKeyWindow),
                accepts_first_responder as extern "C" fn(_, _) -> _,
            );
        }
        decl.register()
    })
}

fn is_escape(event: id) -> bool {
    let code: u16 = unsafe { msg_send![event, keyCode] };
    code == ESCAPE_KEY
}

fn view_class() -> &'static Class {
    static CLASS: OnceLock<&'static Class> = OnceLock::new();
    CLASS.get_or_init(|| {
        let mut decl = ClassBuilder::new(c"CongmiaoCaptureView", class!(NSView))
            .expect("无法注册截图遮罩视图");
        unsafe {
            decl.add_method(sel!(isFlipped), is_flipped as extern "C" fn(_, _) -> _);
            decl.add_method(
                sel!(acceptsFirstResponder),
                accepts_first_responder as extern "C" fn(_, _) -> _,
            );
            decl.add_method(sel!(drawRect:), draw_rect as extern "C" fn(_, _, _));
            decl.add_method(sel!(hitTest:), hit_test as extern "C" fn(_, _, _) -> _);
            decl.add_method(sel!(mouseDown:), mouse_down as extern "C" fn(_, _, _));
            decl.add_method(sel!(mouseDragged:), mouse_dragged as extern "C" fn(_, _, _));
            decl.add_method(sel!(mouseUp:), mouse_up as extern "C" fn(_, _, _));
            decl.add_method(sel!(keyDown:), key_down as extern "C" fn(_, _, _));
        }
        decl.register()
    })
}

extern "C" fn is_flipped(_this: &Object, _cmd: Sel) -> BOOL {
    YES
}

extern "C" fn accepts_first_responder(_this: &Object, _cmd: Sel) -> BOOL {
    YES
}

extern "C" fn hit_test(this: &Object, _cmd: Sel, _point: NSPoint) -> id {
    this as *const Object as id
}

extern "C" fn draw_rect(this: &Object, _cmd: Sel, _dirty: NSRect) {
    let bounds: NSRect = unsafe { msg_send![this, bounds] };
    let selection = SESSION.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|session| rect_from_points(session.anchor, session.current))
    });
    unsafe {
        let shade: id = msg_send![class!(NSColor), colorWithCalibratedWhite: 0.0, alpha: 0.45];
        let _: () = msg_send![shade, setFill];
        NSRectFill(bounds);
        if let Some(rect) = selection {
            let clear: id = msg_send![class!(NSColor), clearColor];
            let _: () = msg_send![clear, setFill];
            NSRectFillUsingOperation(rect, COMPOSITE_COPY);
            let path: id = msg_send![class!(NSBezierPath), bezierPathWithRect: rect];
            let blue: id = msg_send![class!(NSColor), colorWithCalibratedRed: 0.0,
                green: 0.37,
                blue: 0.72,
                alpha: 1.0];
            let _: () = msg_send![blue, setStroke];
            let _: () = msg_send![path, setLineWidth: 2.0f64];
            let _: () = msg_send![path, stroke];
        }
        draw_hint();
    }
}

extern "C" fn mouse_down(this: &Object, _cmd: Sel, event: id) {
    let point = event_point(this, event);
    SESSION.with(|slot| {
        if let Some(session) = slot.borrow_mut().as_mut() {
            session.anchor = Some(point);
            session.current = Some(point);
        }
    });
    redraw();
}

extern "C" fn mouse_dragged(this: &Object, _cmd: Sel, event: id) {
    let point = event_point(this, event);
    SESSION.with(|slot| {
        if let Some(session) = slot.borrow_mut().as_mut() {
            session.current = Some(point);
        }
    });
    redraw();
}

extern "C" fn mouse_up(this: &Object, _cmd: Sel, event: id) {
    let point = event_point(this, event);
    let taken = SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let session = slot.as_mut()?;
        session.current = Some(point);
        let selection = rect_from_points(session.anchor, session.current);
        let window = session.window;
        let app = session.app.clone();
        let session = slot.take()?;
        detach_monitors(&session);
        retire_blocks(&app, session.local_block, session.global_block);
        Some((selection, window, app))
    });
    let Some((selection, window, app)) = taken else {
        return;
    };
    unsafe {
        let _: () = msg_send![window, orderOut: nil];
    }
    pump_run_loop();
    let Some(view_rect) =
        selection.filter(|rect| rect.size.width >= 8.0 && rect.size.height >= 8.0)
    else {
        release_window(window);
        return;
    };
    let screen_rect = view_rect_to_screen(window, view_rect);
    release_window(window);
    if !screen_capture_allowed() {
        show_panel(
            screen_rect,
            "没有截到画面。请在系统设置的「屏幕录制」里允许正在运行的从喵翻译。",
            "",
        );
        return;
    }
    let Some(image) = capture_screen(screen_rect) else {
        show_panel(
            screen_rect,
            "没有截到画面。请在系统设置的「屏幕录制」里允许正在运行的从喵翻译。",
            "",
        );
        return;
    };
    let recognized = recognize_cgimage(image);
    unsafe { CGImageRelease(image) };
    let recognized = match recognized {
        Ok(text) => text,
        Err(message) => {
            show_panel(screen_rect, &message, "");
            return;
        }
    };
    let recognized = recognized.trim().to_string();
    let origin = screen_rect.origin;
    if recognized.is_empty() {
        crate::popup::open(&app, origin.x, origin.y, "没有识别到文字", "notice");
        return;
    }
    if crate::capture::take_silent() {
        copy_plain(&recognized);
        crate::popup::open(&app, origin.x, origin.y, "已复制识别到的文字", "notice");
        return;
    }
    crate::popup::open(&app, origin.x, origin.y, &recognized, "ocr");
}

fn copy_plain(text: &str) {
    crate::clipboard::write_text(text);
}

extern "C" fn key_down(_this: &Object, _cmd: Sel, event: id) {
    let code: u16 = unsafe { msg_send![event, keyCode] };
    if code == ESCAPE_KEY {
        cancel();
    }
}

fn cancel() {
    let session = SESSION.with(|slot| slot.borrow_mut().take());
    let Some(session) = session else {
        return;
    };
    detach_monitors(&session);
    unsafe {
        let _: () = msg_send![session.window, orderOut: nil];
    }
    release_window(session.window);
    retire_blocks(&session.app, session.local_block, session.global_block);
}

fn detach_monitors(session: &Session) {
    unsafe {
        if !session.local_monitor.is_null() {
            let _: () = msg_send![class!(NSEvent), removeMonitor: session.local_monitor];
        }
        if !session.global_monitor.is_null() {
            let _: () = msg_send![class!(NSEvent), removeMonitor: session.global_monitor];
        }
    }
}

struct RetiredBlocks(*mut (RcBlock<dyn Fn(id) -> id>, RcBlock<dyn Fn(id)>));

unsafe impl Send for RetiredBlocks {}

impl RetiredBlocks {
    fn release(self) {
        unsafe {
            drop(Box::from_raw(self.0));
        }
    }
}

fn retire_blocks(app: &AppHandle, local: RcBlock<dyn Fn(id) -> id>, global: RcBlock<dyn Fn(id)>) {
    let retired = RetiredBlocks(Box::into_raw(Box::new((local, global))));
    let _ = app.run_on_main_thread(move || retired.release());
}

fn redraw() {
    let view = SESSION.with(|slot| slot.borrow().as_ref().map(|session| session.view));
    if let Some(view) = view {
        unsafe {
            let _: () = msg_send![view, setNeedsDisplay: YES];
        }
    }
}

fn event_point(view: &Object, event: id) -> NSPoint {
    let window_point: NSPoint = unsafe { msg_send![event, locationInWindow] };
    unsafe { msg_send![view, convertPoint: window_point, fromView: nil] }
}

fn rect_from_points(anchor: Option<NSPoint>, current: Option<NSPoint>) -> Option<NSRect> {
    let (Some(start), Some(end)) = (anchor, current) else {
        return None;
    };
    let x = start.x.min(end.x);
    let y = start.y.min(end.y);
    let width = (start.x - end.x).abs();
    let height = (start.y - end.y).abs();
    if width < 1.0 || height < 1.0 {
        return None;
    }
    Some(NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)))
}

fn view_rect_to_screen(window: id, rect: NSRect) -> NSRect {
    let frame: NSRect = unsafe { msg_send![window, frame] };
    let x = frame.origin.x + rect.origin.x;
    let y = frame.origin.y + frame.size.height - (rect.origin.y + rect.size.height);
    NSRect::new(NSPoint::new(x, y), rect.size)
}

fn union_screen_frame() -> NSRect {
    let screens: id = unsafe { msg_send![class!(NSScreen), screens] };
    let count: usize = unsafe { msg_send![screens, count] };
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;
    for index in 0..count {
        let screen: id = unsafe { msg_send![screens, objectAtIndex: index] };
        let frame: NSRect = unsafe { msg_send![screen, frame] };
        min_x = min_x.min(frame.origin.x);
        min_y = min_y.min(frame.origin.y);
        max_x = max_x.max(frame.origin.x + frame.size.width);
        max_y = max_y.max(frame.origin.y + frame.size.height);
    }
    if count == 0 {
        return NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
    }
    NSRect::new(
        NSPoint::new(min_x, min_y),
        NSSize::new(max_x - min_x, max_y - min_y),
    )
}

pub(crate) fn screen_recording_granted() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() != 0 }
}

pub(crate) fn request_screen_recording() {
    unsafe {
        CGRequestScreenCaptureAccess();
    }
}

fn screen_capture_allowed() -> bool {
    unsafe {
        if CGPreflightScreenCaptureAccess() != 0 {
            return true;
        }
        CGRequestScreenCaptureAccess() != 0
    }
}

fn capture_screen(rect: NSRect) -> Option<*mut c_void> {
    let primary = primary_screen_height();
    let cg = CGRect {
        origin: CGPoint {
            x: rect.origin.x,
            y: primary - (rect.origin.y + rect.size.height),
        },
        size: CGSize {
            width: rect.size.width,
            height: rect.size.height,
        },
    };
    let image = unsafe { CGWindowListCreateImage(cg, 1, 0, 1 << 3) };
    if image.is_null() {
        None
    } else {
        Some(image)
    }
}

fn primary_screen_height() -> f64 {
    let screens: id = unsafe { msg_send![class!(NSScreen), screens] };
    let count: usize = unsafe { msg_send![screens, count] };
    for index in 0..count {
        let screen: id = unsafe { msg_send![screens, objectAtIndex: index] };
        let frame: NSRect = unsafe { msg_send![screen, frame] };
        if frame.origin.x == 0.0 && frame.origin.y == 0.0 {
            return frame.size.height;
        }
    }
    let screen: id = unsafe { msg_send![screens, objectAtIndex: 0usize] };
    let frame: NSRect = unsafe { msg_send![screen, frame] };
    frame.size.height
}

fn recognize_cgimage(image: *mut c_void) -> Result<String, String> {
    unsafe {
        let request: id = msg_send![class!(VNRecognizeTextRequest), new];
        if request.is_null() {
            return Err("无法创建 Vision 识别请求".into());
        }
        let _: () = msg_send![request, setRecognitionLevel: 0usize];
        let _: () = msg_send![request, setUsesLanguageCorrection: YES];
        let _: () = msg_send![request, setAutomaticallyDetectsLanguage: YES];
        let handler: id = msg_send![class!(VNImageRequestHandler), alloc];
        let handler: id = msg_send![handler, initWithCGImage: image, options: nil];
        let requests = ns_array(&[request]);
        let mut error: id = nil;
        let ok: BOOL = msg_send![handler, performRequests: requests, error: &mut error];
        if ok == NO {
            return Err(error_message(error));
        }
        let results: id = msg_send![request, results];
        Ok(join_observations(results))
    }
}

fn join_observations(results: id) -> String {
    if results.is_null() {
        return String::new();
    }
    let count: usize = unsafe { msg_send![results, count] };
    let mut lines = Vec::with_capacity(count);
    for index in 0..count {
        let observation: id = unsafe { msg_send![results, objectAtIndex: index] };
        let candidates: id = unsafe { msg_send![observation, topCandidates: 1usize] };
        let candidate: id = unsafe { msg_send![candidates, firstObject] };
        if candidate.is_null() {
            continue;
        }
        let raw: id = unsafe { msg_send![candidate, string] };
        let text = ns_to_string(raw);
        if text.trim().is_empty() {
            continue;
        }
        let bounds: NSRect = unsafe { msg_send![observation, boundingBox] };
        lines.push((bounds.origin.y, bounds.origin.x, text));
    }
    lines.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                left.1
                    .partial_cmp(&right.1)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    lines
        .into_iter()
        .map(|(_, _, text)| text)
        .collect::<Vec<_>>()
        .join("\n")
}

async fn translate_ocr(text: &str) -> String {
    let target = default_target();
    let paths = match congmiao_core::data_dir() {
        Ok(dir) => congmiao_core::DaemonPaths::new(dir),
        Err(err) => return err.to_string(),
    };
    let endpoint = match congmiao_core::Endpoint::load(&paths.endpoint()) {
        Ok(endpoint) => endpoint,
        Err(_) => return "从喵翻译没有在运行".into(),
    };
    let client = match congmiao_core::DaemonClient::new(&endpoint) {
        Ok(client) => client,
        Err(err) => return err.to_string(),
    };
    match client.translate("auto", &target, text).await {
        Ok(response) => response.text,
        Err(err) => err.to_string(),
    }
}

fn default_target() -> String {
    let Ok(dir) = congmiao_core::data_dir() else {
        return "zh".into();
    };
    congmiao_core::AppConfig::load_from(&congmiao_core::DaemonPaths::new(dir).config())
        .map(|config| config.default_target)
        .unwrap_or_else(|_| "zh".into())
}

pub(crate) fn show_translation(app: &AppHandle, x: f64, y: f64, text: &str) {
    let anchor = NSRect::new(NSPoint::new(x, y), NSSize::new(1.0, 1.0));
    show_panel(anchor, text, "正在翻译…");
    let source = text.to_string();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let translated = translate_ocr(&source).await;
        let _ = app.run_on_main_thread(move || {
            PANEL.with(|slot| {
                let panel = slot.borrow();
                let Some(panel) = panel.as_ref() else {
                    return;
                };
                set_string(panel.body, &translated);
            });
        });
    });
}

pub(crate) fn show_notice(x: f64, y: f64, message: &str) {
    let anchor = NSRect::new(NSPoint::new(x, y), NSSize::new(1.0, 1.0));
    show_panel(anchor, message, "");
}

fn show_panel(screen_rect: NSRect, source: &str, translation: &str) {
    close_panel();
    let width = 360.0;
    let height = 150.0;
    let origin = NSPoint::new(
        screen_rect.origin.x,
        (screen_rect.origin.y - height - 12.0).max(24.0),
    );
    let frame = NSRect::new(origin, NSSize::new(width, height));
    let window: id = unsafe { msg_send![class!(NSPanel), alloc] };
    let window: id = unsafe {
        msg_send![window, initWithContentRect: frame,
            styleMask: 3usize | NONACTIVATING_PANEL,
            backing: 2usize,
            defer: NO]
    };
    let source_field = text_field(
        NSRect::new(NSPoint::new(16.0, 78.0), NSSize::new(width - 32.0, 52.0)),
        source,
        13.0,
    );
    let body = text_field(
        NSRect::new(NSPoint::new(16.0, 16.0), NSSize::new(width - 32.0, 56.0)),
        translation,
        16.0,
    );
    unsafe {
        let content: id = msg_send![window, contentView];
        let _: () = msg_send![content, addSubview: source_field];
        let _: () = msg_send![content, addSubview: body];
        let _: () = msg_send![window, setTitle: ns_string("从喵翻译")];
        let _: () = msg_send![window, setLevel: FLOATING_LEVEL];
        let _: () = msg_send![window, setHidesOnDeactivate: NO];
        let _: () = msg_send![window, setReleasedWhenClosed: NO];
        let _: () = msg_send![window, orderFrontRegardless];
    }
    PANEL.with(|slot| {
        *slot.borrow_mut() = Some(Panel { window, body });
    });
}

fn close_panel() {
    let previous = PANEL.with(|slot| slot.borrow_mut().take());
    if let Some(panel) = previous {
        unsafe {
            let _: () = msg_send![panel.window, orderOut: nil];
        }
    }
}

fn text_field(frame: NSRect, text: &str, size: f64) -> id {
    let field: id = unsafe { msg_send![class!(NSTextField), alloc] };
    let field: id = unsafe { msg_send![field, initWithFrame: frame] };
    unsafe {
        let font: id = msg_send![class!(NSFont), systemFontOfSize: size];
        let _: () = msg_send![field, setStringValue: ns_string(text)];
        let _: () = msg_send![field, setFont: font];
        let _: () = msg_send![field, setEditable: NO];
        let _: () = msg_send![field, setBezeled: NO];
        let _: () = msg_send![field, setDrawsBackground: NO];
        let _: () = msg_send![field, setLineBreakMode: 0usize];
    }
    field
}

fn set_string(field: id, text: &str) {
    if field.is_null() {
        return;
    }
    unsafe {
        let _: () = msg_send![field, setStringValue: ns_string(text)];
    }
}

fn draw_hint() {
    let text = ns_string("拖拽框选要翻译的区域，按 Esc 取消");
    let font: id = unsafe { msg_send![class!(NSFont), systemFontOfSize: 16.0f64] };
    let color: id = unsafe { msg_send![class!(NSColor), whiteColor] };
    let keys = unsafe { ns_array(&[NSFontAttributeName, NSForegroundColorAttributeName]) };
    let values = ns_array(&[font, color]);
    let attrs: id =
        unsafe { msg_send![class!(NSDictionary), dictionaryWithObjects: values, forKeys: keys] };
    let rect = NSRect::new(NSPoint::new(24.0, 28.0), NSSize::new(520.0, 24.0));
    unsafe {
        let _: () = msg_send![text, drawInRect: rect, withAttributes: attrs];
    }
}

fn pump_run_loop() {
    unsafe {
        let run_loop: id = msg_send![class!(NSRunLoop), currentRunLoop];
        let date: id = msg_send![class!(NSDate), dateWithTimeIntervalSinceNow: 0.05f64];
        let _: () = msg_send![run_loop, runUntilDate: date];
    }
}

fn release_window(window: id) {
    if window.is_null() {
        return;
    }
    unsafe {
        let _: () = msg_send![window, close];
    }
}

fn error_message(error: id) -> String {
    if error.is_null() {
        return "Vision 没有识别成功".into();
    }
    let text: id = unsafe { msg_send![error, localizedDescription] };
    let message = ns_to_string(text);
    if message.is_empty() {
        "Vision 没有识别成功".into()
    } else {
        message
    }
}

fn ns_string(text: &str) -> id {
    compat_ns_string(text)
}

fn ns_array(items: &[id]) -> id {
    let array: id = unsafe { msg_send![class!(NSMutableArray), array] };
    for item in items {
        unsafe {
            let _: () = msg_send![array, addObject: *item];
        }
    }
    array
}

fn ns_to_string(text: id) -> String {
    if text.is_null() {
        return String::new();
    }
    unsafe {
        let bytes: *const i8 = msg_send![text, UTF8String];
        if bytes.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(bytes)
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vision_reads_text_drawn_into_an_image() {
        let _app: id = unsafe { msg_send![class!(NSApplication), sharedApplication] };
        let image = draw_hello();
        assert!(!image.is_null(), "位图没有 CGImage");
        let text = recognize_cgimage(image).expect("vision");
        unsafe { CGImageRelease(image) };
        assert!(text.to_lowercase().contains("hello"), "识别结果是 {text}");
    }

    fn draw_hello() -> *mut c_void {
        unsafe {
            let image: id = msg_send![class!(NSImage), alloc];
            let image: id = msg_send![image, initWithSize: NSSize::new(480.0, 160.0)];
            let _: () = msg_send![image, lockFocus];
            let white: id = msg_send![class!(NSColor), whiteColor];
            let _: () = msg_send![white, set];
            NSRectFill(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(480.0, 160.0),
            ));
            let font: id = msg_send![class!(NSFont), systemFontOfSize: 72.0f64];
            let black: id = msg_send![class!(NSColor), blackColor];
            let keys = ns_array(&[
                NSFontAttributeName as id,
                NSForegroundColorAttributeName as id,
            ]);
            let values = ns_array(&[font, black]);
            let attrs: id =
                msg_send![class!(NSDictionary), dictionaryWithObjects: values, forKeys: keys];
            let text = ns_string("hello");
            let _: () =
                msg_send![text, drawAtPoint: NSPoint::new(24.0, 40.0), withAttributes: attrs];
            let _: () = msg_send![image, unlockFocus];
            let tiff: id = msg_send![image, TIFFRepresentation];
            let rep: id = msg_send![class!(NSBitmapImageRep), imageRepWithData: tiff];
            assert!(!rep.is_null(), "无法从绘制结果得到位图");
            let cg: *mut c_void = msg_send![rep, CGImage];
            CFRetain(cg);
            cg
        }
    }
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    static NSFontAttributeName: id;
    static NSForegroundColorAttributeName: id;
}

#[cfg(test)]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRetain(cf: *const c_void) -> *const c_void;
}
