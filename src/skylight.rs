//! Thin FFI wrapper around the private SkyLight.framework.
//!
//! Reads space focus state directly from the window server — the same way
//! YabaiIndicator does — so updates are instant without querying yabai.

use std::ffi::c_void;
use std::sync::OnceLock;

// ── Raw CoreFoundation types ──────────────────────────────────────────────────

#[allow(non_camel_case_types)]
type CFTypeRef = *const c_void;
#[allow(non_camel_case_types)]
type CFArrayRef = *const c_void;
#[allow(non_camel_case_types)]
type CFDictionaryRef = *const c_void;
#[allow(non_camel_case_types)]
type CFStringRef = *const c_void;
#[allow(non_camel_case_types)]
type CFIndex = isize;
#[allow(non_camel_case_types)]
type CGSConnectionID = i32;

// ── CoreFoundation extern functions ──────────────────────────────────────────

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, idx: CFIndex) -> CFTypeRef;
    fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    fn CFStringGetCString(
        the_string: CFStringRef,
        buffer: *mut u8,
        buffer_size: CFIndex,
        encoding: u32,
    ) -> bool;
    fn CFNumberGetValue(number: CFTypeRef, the_type: i32, value_ptr: *mut c_void) -> bool;
    fn CFRelease(cf: CFTypeRef);
    fn CFStringCreateWithCString(
        alloc: CFTypeRef,
        c_str: *const u8,
        encoding: u32,
    ) -> CFStringRef;
}

// kCFStringEncodingUTF8 = 0x08000100
const UTF8: u32 = 0x0800_0100;
// kCFNumberSInt64Type = 4
const CF_NUMBER_SINT64: i32 = 4;

// ── SkyLight dynamic loading ──────────────────────────────────────────────────

const SKYLIGHT: &std::ffi::CStr =
    c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight";

struct SendPtr(*mut c_void);
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

unsafe fn skylight_fn(name: &std::ffi::CStr) -> *mut c_void {
    static LIB: OnceLock<SendPtr> = OnceLock::new();
    let handle = LIB.get_or_init(|| unsafe {
        SendPtr(libc::dlopen(SKYLIGHT.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL))
    });
    assert!(!handle.0.is_null(), "dlopen SkyLight failed");
    libc::dlsym(handle.0, name.as_ptr())
}

fn sls_connection() -> CGSConnectionID {
    static CONN: OnceLock<CGSConnectionID> = OnceLock::new();
    *CONN.get_or_init(|| unsafe {
        let f = skylight_fn(c"SLSMainConnectionID");
        let f: unsafe extern "C" fn() -> CGSConnectionID = std::mem::transmute(f);
        f()
    })
}

// ── Public API ────────────────────────────────────────────────────────────────

/// One space as reported by SkyLight.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SLSpace {
    pub id64: u64,
    pub display_index: usize, // 0-based display index
    pub is_current: bool,     // visible on its display
    pub is_active: bool,      // visible AND on the active (menu-bar) display
}

/// Read the current space layout directly from SkyLight.
/// Returns spaces in display order. Instant — no IPC with yabai needed.
pub fn query_spaces() -> Vec<SLSpace> {
    unsafe { read_spaces() }
}

unsafe fn read_spaces() -> Vec<SLSpace> {
    let cid = sls_connection();

    // SLSCopyManagedDisplaySpaces(cid) -> CFArray of display dicts
    let copy_displays: unsafe extern "C" fn(CGSConnectionID) -> CFArrayRef = {
        let sym = skylight_fn(c"SLSCopyManagedDisplaySpaces");
        if sym.is_null() {
            return Vec::new();
        }
        std::mem::transmute(sym)
    };

    // SLSCopyActiveMenuBarDisplayIdentifier(cid) -> CFString (UUID of the
    // display whose menu bar is active, i.e. where keyboard focus is)
    let copy_active_display: unsafe extern "C" fn(CGSConnectionID) -> CFStringRef = {
        let sym = skylight_fn(c"SLSCopyActiveMenuBarDisplayIdentifier");
        if sym.is_null() {
            return Vec::new();
        }
        std::mem::transmute(sym)
    };

    let displays = copy_displays(cid);
    if displays.is_null() {
        return Vec::new();
    }

    let active_uuid_ref = copy_active_display(cid);
    let active_uuid = cfstring_to_string(active_uuid_ref);
    if !active_uuid_ref.is_null() {
        CFRelease(active_uuid_ref);
    }

    let display_count = CFArrayGetCount(displays);
    let mut result = Vec::new();

    for di in 0..display_count {
        let display_dict = CFArrayGetValueAtIndex(displays, di);

        // Display UUID
        let disp_uuid_key = make_cfstring("Display Identifier");
        let disp_uuid_ref = CFDictionaryGetValue(display_dict, disp_uuid_key);
        let disp_uuid = cfstring_to_string(disp_uuid_ref);
        CFRelease(disp_uuid_key);

        let is_active_display = active_uuid
            .as_deref()
            .map_or(di == 0, |a| Some(a) == disp_uuid.as_deref());

        // Current space id64 for this display
        let current_key = make_cfstring("Current Space");
        let current_space_dict = CFDictionaryGetValue(display_dict, current_key);
        CFRelease(current_key);
        let current_id = dict_u64(current_space_dict, "id64");

        // All spaces on this display
        let spaces_key = make_cfstring("Spaces");
        let spaces_array = CFDictionaryGetValue(display_dict, spaces_key);
        CFRelease(spaces_key);

        if spaces_array.is_null() {
            continue;
        }

        let space_count = CFArrayGetCount(spaces_array);
        for si in 0..space_count {
            let space_dict = CFArrayGetValueAtIndex(spaces_array, si);
            let id64 = dict_u64(space_dict, "id64").unwrap_or(0);
            let is_current = current_id.map_or(false, |c| c == id64);

                result.push(SLSpace {
                    id64,
                    display_index: di as usize,
                    is_current,
                    is_active: is_current && is_active_display,
                });
        }
    }

    CFRelease(displays);
    result
}

// ── CF helpers ────────────────────────────────────────────────────────────────

unsafe fn make_cfstring(s: &str) -> CFStringRef {
    let mut buf = s.as_bytes().to_vec();
    buf.push(0);
    CFStringCreateWithCString(std::ptr::null(), buf.as_ptr(), UTF8)
}

unsafe fn cfstring_to_string(s: CFStringRef) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let mut buf = vec![0u8; 256];
    if CFStringGetCString(s, buf.as_mut_ptr(), buf.len() as CFIndex, UTF8) {
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).into_owned())
    } else {
        None
    }
}

unsafe fn dict_u64(dict: CFDictionaryRef, key: &str) -> Option<u64> {
    if dict.is_null() {
        return None;
    }
    let cf_key = make_cfstring(key);
    let val = CFDictionaryGetValue(dict, cf_key);
    CFRelease(cf_key);
    if val.is_null() {
        return None;
    }
    let mut out: i64 = 0;
    if CFNumberGetValue(val, CF_NUMBER_SINT64, &mut out as *mut i64 as *mut c_void) {
        Some(out as u64)
    } else {
        None
    }
}
