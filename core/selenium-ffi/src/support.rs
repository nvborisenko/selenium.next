//! Handwritten helpers used by the generated exports.

use selenium_sync as core;
use selenium_sync::{Error, ErrorKind};
use std::cell::RefCell;
use std::ffi::{c_void, CStr, CString};
use std::os::raw::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};

const STATUS_PANIC: i32 = -1;

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_last_error(message: &str) {
    let c = CString::new(message.replace('\0', " ")).unwrap_or_default();
    LAST_ERROR.with(|e| *e.borrow_mut() = c);
}

/// Runs an FFI body: converts errors and panics into status codes and records the message.
pub fn guard<F: FnOnce() -> core::Result<()>>(f: F) -> i32 {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => 0,
        Ok(Err(e)) => {
            set_last_error(&e.message);
            crate::generated::status_of(e.kind)
        }
        Err(_) => {
            set_last_error("internal error (panic) in native library");
            STATUS_PANIC
        }
    }
}

pub fn null_arg() -> Error {
    Error::new(ErrorKind::InvalidArgument, "unexpected null argument")
}

pub unsafe fn handle_ref<'a, T>(p: *mut T) -> core::Result<&'a T> {
    p.as_ref().ok_or_else(null_arg)
}

pub unsafe fn out_ref<'a, T>(p: *mut T) -> core::Result<&'a mut T> {
    p.as_mut().ok_or_else(null_arg)
}

pub unsafe fn cstr<'a>(p: *const c_char) -> core::Result<&'a str> {
    if p.is_null() {
        return Err(null_arg());
    }
    CStr::from_ptr(p)
        .to_str()
        .map_err(|_| Error::new(ErrorKind::InvalidArgument, "string is not valid UTF-8"))
}

pub unsafe fn opt_cstr<'a>(p: *const c_char) -> core::Result<Option<&'a str>> {
    if p.is_null() {
        Ok(None)
    } else {
        cstr(p).map(Some)
    }
}

pub fn into_cstring(s: String) -> *mut c_char {
    CString::new(s.replace('\0', " ")).unwrap_or_default().into_raw()
}

pub fn leak_array<T>(items: Vec<*mut T>) -> (*mut *mut T, usize) {
    let boxed = items.into_boxed_slice();
    let len = boxed.len();
    (Box::into_raw(boxed) as *mut *mut T, len)
}

#[no_mangle]
pub extern "C" fn se_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

#[no_mangle]
pub unsafe extern "C" fn se_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

#[no_mangle]
pub unsafe extern "C" fn se_array_free(items: *mut c_void, len: usize) {
    if !items.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(items as *mut *mut c_void, len)));
    }
}
