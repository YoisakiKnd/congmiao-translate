use std::ffi::{c_void, CStr};

use tauri::AppHandle;

use crate::objc_compat::{
    class, id, msg_send, nil, ns_string as compat_ns_string, NSPoint, NSRect, BOOL, YES,
};

use crate::capture;

const AX_SUCCESS: i32 = 0;
const AX_API_DISABLED: i32 = -25211;
const AX_RECT: u32 = 3;

#[repr(C)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: id) -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
    fn AXUIElementCopyAttributeValue(element: *mut c_void, attribute: id, value: *mut id) -> i32;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: *mut c_void,
        attribute: id,
        parameter: id,
        value: *mut id,
    ) -> i32;
    fn AXUIElementSetAttributeValue(element: *mut c_void, attribute: id, value: id) -> i32;
    fn AXValueGetValue(value: id, value_type: u32, out: *mut c_void) -> u8;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceCreate(state_id: i32) -> *mut c_void;
    fn CGEventSourceFlagsState(state_id: i32) -> u64;
    fn CGEventCreateKeyboardEvent(
        source: *mut c_void,
        key_code: u16,
        key_down: bool,
    ) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: *const c_void;
    fn CFRelease(cf: *const c_void);
}

const MAX_AX_NODES: i32 = 160;
const MAX_AX_DEPTH: i32 = 6;
const COMMAND_FLAG: u64 = 0x0010_0000;
const MODIFIER_MASK: u64 = 0x0002_0000 | 0x0004_0000 | 0x0008_0000 | 0x0010_0000;

pub fn begin(app: &AppHandle) {
    let app_for_task = app.clone();
    let _ = app.run_on_main_thread(move || translate_selection(&app_for_task));
}

pub fn replace(app: &AppHandle) {
    let app_for_task = app.clone();
    let _ = app.run_on_main_thread(move || replace_selection(&app_for_task));
}

pub(crate) fn accessibility_granted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

pub(crate) fn request_accessibility() {
    let _ = accessibility_trusted();
}

fn replace_selection(app: &AppHandle) {
    if !accessibility_trusted() {
        let (x, y) = mouse_point();
        capture::show_notice(
            app,
            x,
            y,
            "请在系统设置的「辅助功能」里允许正在运行的从喵翻译，然后重新选中文字。",
        );
        return;
    }
    let (x, y) = anchor_point();
    let text = match selected_text() {
        Ok(Some(text)) => trim_selection(&text),
        Ok(None) => String::new(),
        Err(message) => {
            capture::show_notice(app, x, y, &message);
            return;
        }
    };
    if text.is_empty() {
        capture::show_notice(app, x, y, "没有选中文字");
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let translated = translate_for_replace(&text).await;
        let _ = app.run_on_main_thread(move || paste_translation(&translated));
    });
}

async fn translate_for_replace(text: &str) -> String {
    let target = default_target();
    let Ok(dir) = congmiao_core::data_dir() else {
        return text.to_string();
    };
    let Ok(endpoint) =
        congmiao_core::Endpoint::load(&congmiao_core::DaemonPaths::new(dir).endpoint())
    else {
        return text.to_string();
    };
    let Ok(client) = congmiao_core::DaemonClient::new(&endpoint) else {
        return text.to_string();
    };
    client
        .translate("auto", &target, text)
        .await
        .map(|response| response.text)
        .unwrap_or_else(|_| text.to_string())
}

fn default_target() -> String {
    let Ok(dir) = congmiao_core::data_dir() else {
        return "zh".into();
    };
    congmiao_core::AppConfig::load_from(&congmiao_core::DaemonPaths::new(dir).config())
        .map(|config| config.default_target)
        .unwrap_or_else(|_| "zh".into())
}

fn paste_translation(text: &str) {
    wait_for_shortcut_release();
    let pasteboard: id = unsafe { msg_send![class!(NSPasteboard), generalPasteboard] };
    let backup = backup_clipboard(pasteboard);
    crate::clipboard::write_text(text);
    post_command_v();
    pump(0.2);
    restore_clipboard(pasteboard, backup);
}

fn post_command_v() {
    unsafe {
        let source = CGEventSourceCreate(1);
        let down = CGEventCreateKeyboardEvent(source, 9, true);
        let up = CGEventCreateKeyboardEvent(source, 9, false);
        CGEventSetFlags(down, COMMAND_FLAG);
        CGEventSetFlags(up, COMMAND_FLAG);
        CGEventPost(0, down);
        CGEventPost(0, up);
        release_cf(down);
        release_cf(up);
        release_cf(source);
    }
}

fn translate_selection(app: &AppHandle) {
    if !accessibility_trusted() {
        let (x, y) = mouse_point();
        capture::show_notice(
            app,
            x,
            y,
            "请在系统设置的「辅助功能」里允许正在运行的从喵翻译，然后重新选中文字。",
        );
        return;
    }
    let (x, y) = anchor_point();
    match selected_text() {
        Ok(Some(text)) => {
            let text = trim_selection(&text);
            if text.is_empty() {
                capture::show_notice(app, x, y, "没有选中文字");
            } else {
                capture::show_translation(app, x, y, &text);
            }
        }
        Ok(None) => capture::show_notice(app, x, y, "没有选中文字"),
        Err(message) => capture::show_notice(app, x, y, &message),
    }
}

fn accessibility_trusted() -> bool {
    let key = ns_string("AXTrustedCheckOptionPrompt");
    let value: id = unsafe { msg_send![class!(NSNumber), numberWithBool: YES] };
    let options: id =
        unsafe { msg_send![class!(NSDictionary), dictionaryWithObject: value, forKey: key] };
    unsafe { AXIsProcessTrustedWithOptions(options) != 0 }
}

fn selected_text() -> Result<Option<String>, String> {
    let pid = frontmost_pid()?;
    let application = unsafe { AXUIElementCreateApplication(pid) };
    if application.is_null() {
        return Err("没有找到前台应用".into());
    }
    if enable_enhanced_accessibility(application) {
        pump(0.12);
    }
    let focused = copy_attribute(application, "AXFocusedUIElement");
    if let Err(AX_API_DISABLED) = focused {
        unsafe { CFRelease(application) };
        return Err(ax_message(AX_API_DISABLED));
    }
    let mut found = None;
    if let Ok(element) = focused {
        let element = element as *mut c_void;
        found = text_from_element(element)
            .or_else(|| text_from_ancestors(element))
            .or_else(|| {
                let mut seen = 0;
                find_selected(element, 0, &mut seen)
            });
        unsafe { CFRelease(element) };
    }
    unsafe { CFRelease(application) };
    if found.is_some() {
        return Ok(found);
    }
    Ok(copy_selection_with_shortcut())
}

fn enable_enhanced_accessibility(application: *mut c_void) -> bool {
    let name = ns_string("AXEnhancedUserInterface");
    let err = unsafe { AXUIElementSetAttributeValue(application, name, kCFBooleanTrue as id) };
    release(name);
    err == AX_SUCCESS
}

fn text_from_ancestors(element: *mut c_void) -> Option<String> {
    let mut current = copy_attribute(element, "AXParent").ok()?;
    for _ in 0..16 {
        if let Some(text) = text_from_element(current as *mut c_void) {
            release(current);
            return Some(text);
        }
        let parent = copy_attribute(current as *mut c_void, "AXParent").ok();
        release(current);
        current = parent?;
    }
    release(current);
    None
}

fn text_from_element(element: *mut c_void) -> Option<String> {
    let value = copy_attribute(element, "AXSelectedText").ok()?;
    let text = ns_to_string(value);
    release(value);
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn find_selected(element: *mut c_void, depth: i32, seen: &mut i32) -> Option<String> {
    if element.is_null() || depth > MAX_AX_DEPTH || *seen >= MAX_AX_NODES {
        return None;
    }
    *seen += 1;
    if let Some(text) = text_from_element(element) {
        return Some(text);
    }
    let children = match copy_attribute(element, "AXChildren") {
        Ok(children) => children,
        Err(_) => return None,
    };
    let count: usize = unsafe { msg_send![children, count] };
    let mut found = None;
    for index in 0..count {
        if found.is_some() || *seen >= MAX_AX_NODES {
            break;
        }
        let child: id = unsafe { msg_send![children, objectAtIndex: index] };
        found = find_selected(child as *mut c_void, depth + 1, seen);
    }
    release(children);
    found
}

fn copy_selection_with_shortcut() -> Option<String> {
    wait_for_shortcut_release();
    let pasteboard: id = unsafe { msg_send![class!(NSPasteboard), generalPasteboard] };
    let before: isize = unsafe { msg_send![pasteboard, changeCount] };
    let backup = backup_clipboard(pasteboard);
    post_command_c();
    let mut copied = None;
    for _ in 0..12 {
        pump(0.04);
        let count: isize = unsafe { msg_send![pasteboard, changeCount] };
        if count != before {
            let value: id = unsafe {
                msg_send![pasteboard, stringForType: ns_string("public.utf8-plain-text")]
            };
            let text = ns_to_string(value);
            if !text.trim().is_empty() {
                copied = Some(text);
                break;
            }
        }
    }
    restore_clipboard(pasteboard, backup);
    copied
}

fn wait_for_shortcut_release() {
    for _ in 0..16 {
        let flags = unsafe { CGEventSourceFlagsState(1) };
        if flags & MODIFIER_MASK == 0 {
            return;
        }
        pump(0.04);
    }
}

fn post_command_c() {
    unsafe {
        let source = CGEventSourceCreate(1);
        let down = CGEventCreateKeyboardEvent(source, 8, true);
        let up = CGEventCreateKeyboardEvent(source, 8, false);
        CGEventSetFlags(down, COMMAND_FLAG);
        CGEventSetFlags(up, COMMAND_FLAG);
        CGEventPost(0, down);
        CGEventPost(0, up);
        release_cf(down);
        release_cf(up);
        release_cf(source);
    }
}

fn backup_clipboard(pasteboard: id) -> id {
    let items: id = unsafe { msg_send![pasteboard, pasteboardItems] };
    let archive: id = unsafe { msg_send![class!(NSMutableArray), array] };
    let _: id = unsafe { msg_send![archive, retain] };
    if items.is_null() {
        return archive;
    }
    let count: usize = unsafe { msg_send![items, count] };
    for index in 0..count {
        let item: id = unsafe { msg_send![items, objectAtIndex: index] };
        let copy: id = unsafe { msg_send![class!(NSPasteboardItem), new] };
        let types: id = unsafe { msg_send![item, types] };
        if types.is_null() {
            continue;
        }
        let type_count: usize = unsafe { msg_send![types, count] };
        for type_index in 0..type_count {
            let kind: id = unsafe { msg_send![types, objectAtIndex: type_index] };
            let data: id = unsafe { msg_send![item, dataForType: kind] };
            if !data.is_null() {
                let _: BOOL = unsafe { msg_send![copy, setData: data, forType: kind] };
            }
        }
        let _: () = unsafe { msg_send![archive, addObject: copy] };
    }
    archive
}

fn restore_clipboard(pasteboard: id, backup: id) {
    unsafe {
        let _: () = msg_send![pasteboard, clearContents];
        let count: usize = msg_send![backup, count];
        if count > 0 {
            let _: BOOL = msg_send![pasteboard, writeObjects: backup];
        }
        let _: () = msg_send![backup, release];
    }
}

fn pump(seconds: f64) {
    unsafe {
        let run_loop: id = msg_send![class!(NSRunLoop), currentRunLoop];
        let date: id = msg_send![class!(NSDate), dateWithTimeIntervalSinceNow: seconds];
        let _: () = msg_send![run_loop, runUntilDate: date];
    }
}

fn release(value: id) {
    release_cf(value as *mut c_void);
}

fn release_cf(value: *mut c_void) {
    if !value.is_null() {
        unsafe { CFRelease(value) };
    }
}

fn frontmost_pid() -> Result<i32, String> {
    let workspace: id = unsafe { msg_send![class!(NSWorkspace), sharedWorkspace] };
    let application: id = unsafe { msg_send![workspace, frontmostApplication] };
    if application.is_null() {
        return Err("没有找到前台应用".into());
    }
    let pid: i32 = unsafe { msg_send![application, processIdentifier] };
    Ok(pid)
}

fn copy_attribute(element: *mut c_void, name: &str) -> Result<id, i32> {
    let mut value: id = nil;
    let attribute = ns_string(name);
    let err = unsafe { AXUIElementCopyAttributeValue(element, attribute, &mut value) };
    release(attribute);
    if err != AX_SUCCESS || value.is_null() {
        Err(err)
    } else {
        Ok(value)
    }
}

fn anchor_point() -> (f64, f64) {
    selection_origin().unwrap_or_else(mouse_point)
}

fn mouse_point() -> (f64, f64) {
    let mouse: NSPoint = unsafe { msg_send![class!(NSEvent), mouseLocation] };
    (mouse.x, mouse.y)
}

fn selection_origin() -> Option<(f64, f64)> {
    let pid = frontmost_pid().ok()?;
    let application = unsafe { AXUIElementCreateApplication(pid) };
    if application.is_null() {
        return None;
    }
    let focused = copy_attribute(application, "AXFocusedUIElement").ok();
    unsafe { CFRelease(application) };
    let focused = focused?;
    let range = copy_attribute(focused as *mut c_void, "AXSelectedTextRange").ok();
    if range.is_none() {
        unsafe { CFRelease(focused as *mut c_void) };
        return None;
    }
    let range = range?;
    let mut bounds: id = nil;
    let err = unsafe {
        AXUIElementCopyParameterizedAttributeValue(
            focused as *mut c_void,
            ns_string("AXBoundsForRange"),
            range,
            &mut bounds,
        )
    };
    unsafe {
        CFRelease(range as *mut c_void);
        CFRelease(focused as *mut c_void);
    }
    if err != AX_SUCCESS || bounds.is_null() {
        return None;
    }
    let mut rect = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: 0.0,
            height: 0.0,
        },
    };
    let ok = unsafe { AXValueGetValue(bounds, AX_RECT, &mut rect as *mut CGRect as *mut c_void) };
    unsafe { CFRelease(bounds as *mut c_void) };
    if ok == 0 || rect.size.width < 1.0 || rect.size.height < 1.0 {
        return None;
    }
    let screens: id = unsafe { msg_send![class!(NSScreen), screens] };
    let screen: id = unsafe { msg_send![screens, objectAtIndex: 0usize] };
    let frame: NSRect = unsafe { msg_send![screen, frame] };
    let y = frame.size.height - (rect.origin.y + rect.size.height);
    Some((rect.origin.x, y))
}

fn trim_selection(text: &str) -> String {
    let trimmed = text.trim();
    trimmed.chars().take(8_000).collect()
}

fn ax_message(code: i32) -> String {
    if code == AX_API_DISABLED {
        "请在系统设置的「辅助功能」里允许正在运行的从喵翻译，然后重新选中文字。".into()
    } else {
        format!("没有读到选中的文字（{code}）")
    }
}

fn ns_string(text: &str) -> id {
    compat_ns_string(text)
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
        CStr::from_ptr(bytes).to_string_lossy().into_owned()
    }
}
