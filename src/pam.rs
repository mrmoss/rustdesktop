//! Minimal PAM password check for the lock screen (same approach as swaylock).
//! Links against the runtime libpam.so.0 directly, so no -dev package needed.
//! Service name "rusttouch" (/etc/pam.d/rusttouch, installed by install.sh); if that
//! file is missing PAM falls back to /etc/pam.d/other, which on Ubuntu also
//! uses common-auth.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_CONV_ERR: c_int = 19;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;

#[repr(C)]
#[allow(dead_code)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
#[allow(dead_code)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type ConvFn = unsafe extern "C" fn(
    c_int,
    *mut *const PamMessage,
    *mut *mut PamResponse,
    *mut c_void,
) -> c_int;

#[repr(C)]
struct PamConv {
    conv: Option<ConvFn>,
    appdata_ptr: *mut c_void,
}

#[link(name = "libpam.so.0", kind = "dylib", modifiers = "+verbatim")]
extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conv: *const PamConv,
        pamh: *mut *mut c_void,
    ) -> c_int;
    fn pam_authenticate(pamh: *mut c_void, flags: c_int) -> c_int;
    fn pam_end(pamh: *mut c_void, status: c_int) -> c_int;
}

extern "C" {
    fn calloc(n: usize, size: usize) -> *mut c_void;
    fn strdup(s: *const c_char) -> *mut c_char;
}

/// PAM asks questions; we answer every prompt with the password.
/// PAM frees the replies with free(), so they must come from malloc.
unsafe extern "C" fn conv(
    n: c_int,
    msgs: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    data: *mut c_void,
) -> c_int {
    if n <= 0 || n > 32 {
        return PAM_CONV_ERR;
    }
    let replies = calloc(n as usize, std::mem::size_of::<PamResponse>()) as *mut PamResponse;
    if replies.is_null() {
        return PAM_BUF_ERR;
    }
    let password = data as *const c_char;
    for i in 0..n as isize {
        let m = *msgs.offset(i);
        if m.is_null() {
            continue;
        }
        let style = (*m).msg_style;
        if style == PAM_PROMPT_ECHO_OFF || style == PAM_PROMPT_ECHO_ON {
            (*replies.offset(i)).resp = strdup(password);
        }
    }
    *resp = replies;
    PAM_SUCCESS
}

/// Blocking (PAM sleeps ~2s on failure) - call from a worker thread.
pub fn authenticate(user: &str, password: &str) -> bool {
    let (Ok(service), Ok(user), Ok(pw)) = (
        CString::new("rusttouch"),
        CString::new(user),
        CString::new(password),
    ) else {
        return false;
    };
    let c = PamConv {
        conv: Some(conv),
        appdata_ptr: pw.as_ptr() as *mut c_void,
    };
    let mut h: *mut c_void = ptr::null_mut();
    unsafe {
        if pam_start(service.as_ptr(), user.as_ptr(), &c, &mut h) != PAM_SUCCESS {
            return false;
        }
        let rc = pam_authenticate(h, 0);
        pam_end(h, rc);
        rc == PAM_SUCCESS
    }
}
