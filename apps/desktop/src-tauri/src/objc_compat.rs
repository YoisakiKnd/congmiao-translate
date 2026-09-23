use std::ffi::CString;

use objc2::runtime::{AnyObject, Bool};

pub use objc2::runtime::{AnyClass as Class, AnyObject as Object, Bool as BOOL, Sel};
pub use objc2::{class, msg_send, sel};
pub use objc2_core_foundation::{CGPoint as NSPoint, CGRect as NSRect, CGSize as NSSize};

#[allow(non_camel_case_types)]
pub type id = *mut AnyObject;

#[allow(non_upper_case_globals)]
pub const nil: id = std::ptr::null_mut();
pub const YES: BOOL = Bool::YES;
pub const NO: BOOL = Bool::NO;

pub fn ns_string(text: &str) -> id {
    let cstring = CString::new(text).unwrap_or_else(|_| CString::new("").expect("empty"));
    unsafe { msg_send![class!(NSString), stringWithUTF8String: cstring.as_ptr()] }
}
