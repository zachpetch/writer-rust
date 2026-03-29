// Minimal Objective-C runtime and macOS framework bindings.
//
// This module provides the raw FFI layer for communicating with the Objective-C
// runtime. On macOS, all Cocoa APIs (AppKit, Foundation) are Objective-C classes.
// To call them from Rust, we use:
//
// 1. `objc_getClass` - look up a class by name (e.g., "NSWindow")
// 2. `sel_registerName` - look up a selector by name (e.g., "alloc")
// 3. `objc_msgSend` - send a message (call a method) on an object
//
// `objc_msgSend` is a trampoline: it looks up the method implementation for the
// given selector on the object's class and tail-calls it. Because it has no fixed
// signature, we transmute its address to a concrete function pointer type for each
// call site. This is safe as long as the types match what the Objective-C method
// actually expects.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// The raw address of `objc_msgSend`, used for transmuting to typed function pointers.
/// We go through `*const ()` to satisfy Rust 2024's lint on function-to-integer casts.
pub fn msg_addr() -> usize {
    objc_msgSend as *const () as usize
}

pub type Id = *mut std::ffi::c_void;
pub type Class = *mut std::ffi::c_void;
pub type Sel = *mut std::ffi::c_void;
pub type BOOL = i8;
pub type IMP = *const std::ffi::c_void;
pub type NSUInteger = u64;
pub type NSInteger = i64;
pub type CGFloat = f64;

pub const NIL: Id = std::ptr::null_mut();
pub const YES: BOOL = 1;
pub const NO: BOOL = 0;

// --- Geometry types (matching CGGeometry / NSGeometry) ---

#[repr(C)]
#[derive(Copy, Clone)]
pub struct NSPoint {
    pub x: CGFloat,
    pub y: CGFloat,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct NSSize {
    pub width: CGFloat,
    pub height: CGFloat,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct NSRect {
    pub origin: NSPoint,
    pub size: NSSize,
}

impl NSRect {
    pub fn new(x: CGFloat, y: CGFloat, w: CGFloat, h: CGFloat) -> Self {
        NSRect {
            origin: NSPoint { x, y },
            size: NSSize { width: w, height: h },
        }
    }
}

impl NSSize {
    pub fn new(w: CGFloat, h: CGFloat) -> Self {
        NSSize { width: w, height: h }
    }
}

// --- Objective-C runtime functions ---

#[link(name = "objc", kind = "dylib")]
unsafe extern "C" {
    pub fn objc_getClass(name: *const c_char) -> Class;
    pub fn objc_msgSend();
    pub fn sel_registerName(name: *const c_char) -> Sel;
    pub fn objc_allocateClassPair(
        superclass: Class,
        name: *const c_char,
        extra_bytes: usize,
    ) -> Class;
    pub fn objc_registerClassPair(cls: Class);
    pub fn class_addMethod(cls: Class, sel: Sel, imp: IMP, types: *const c_char) -> BOOL;
}

// Link frameworks (no functions imported directly; we call everything via objc_msgSend)
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {}

// --- Helpers ---

/// Look up an Objective-C selector by name.
pub fn sel(name: &str) -> Sel {
    let c = CString::new(name).unwrap();
    unsafe { sel_registerName(c.as_ptr()) }
}

/// Look up an Objective-C class by name.
pub fn cls(name: &str) -> Class {
    let c = CString::new(name).unwrap();
    unsafe { objc_getClass(c.as_ptr()) }
}

/// Create an `NSString` from a Rust `&str`.
pub fn nsstring(s: &str) -> Id {
    unsafe {
        let nscls = cls("NSString");
        let alloc: extern "C" fn(Id, Sel) -> Id =
            std::mem::transmute(msg_addr());
        let obj = alloc(nscls, sel("alloc"));

        let init: extern "C" fn(Id, Sel, *const u8, NSUInteger, NSUInteger) -> Id =
            std::mem::transmute(msg_addr());
        // NSUTF8StringEncoding = 4
        init(
            obj,
            sel("initWithBytes:length:encoding:"),
            s.as_ptr(),
            s.len() as NSUInteger,
            4,
        )
    }
}

/// Read the contents of an `NSString` into a Rust `String`.
pub fn nsstring_to_string(nsstr: Id) -> String {
    unsafe {
        let utf8: extern "C" fn(Id, Sel) -> *const c_char =
            std::mem::transmute(msg_addr());
        let ptr = utf8(nsstr, sel("UTF8String"));
        if ptr.is_null() {
            String::new()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

// --- Typed objc_msgSend wrappers ---
//
// Each wrapper transmutes `objc_msgSend` to the exact function-pointer type
// needed for a particular call pattern. This is how all Objective-C method
// calls happen from Rust.

/// `[obj sel]` -> Id
pub unsafe fn msg(obj: Id, s: Sel) -> Id {
    unsafe {
        let f: extern "C" fn(Id, Sel) -> Id = std::mem::transmute(msg_addr());
        f(obj, s)
    }
}

/// `[obj sel:arg]` -> Id  (one Id argument)
pub unsafe fn msg1(obj: Id, s: Sel, a: Id) -> Id {
    unsafe {
        let f: extern "C" fn(Id, Sel, Id) -> Id = std::mem::transmute(msg_addr());
        f(obj, s, a)
    }
}

/// `[obj sel]` -> void
pub unsafe fn msg_v(obj: Id, s: Sel) {
    unsafe {
        let f: extern "C" fn(Id, Sel) = std::mem::transmute(msg_addr());
        f(obj, s)
    }
}

/// `[obj sel:arg]` -> void  (one Id argument)
pub unsafe fn msg1_v(obj: Id, s: Sel, a: Id) {
    unsafe {
        let f: extern "C" fn(Id, Sel, Id) = std::mem::transmute(msg_addr());
        f(obj, s, a)
    }
}

/// `[obj sel:val]` -> void  (one BOOL argument)
pub unsafe fn msg_bool_v(obj: Id, s: Sel, v: BOOL) {
    unsafe {
        let f: extern "C" fn(Id, Sel, BOOL) = std::mem::transmute(msg_addr());
        f(obj, s, v)
    }
}

/// `[obj sel:val]` -> void  (one NSUInteger argument)
pub unsafe fn msg_uint_v(obj: Id, s: Sel, v: NSUInteger) {
    unsafe {
        let f: extern "C" fn(Id, Sel, NSUInteger) = std::mem::transmute(msg_addr());
        f(obj, s, v)
    }
}

/// `[obj sel:val]` -> Id  (one NSUInteger argument)
pub unsafe fn msg_uint(obj: Id, s: Sel, v: NSUInteger) -> Id {
    unsafe {
        let f: extern "C" fn(Id, Sel, NSUInteger) -> Id = std::mem::transmute(msg_addr());
        f(obj, s, v)
    }
}
