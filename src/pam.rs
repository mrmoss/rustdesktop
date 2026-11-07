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
    fn free(p: *mut c_void);
    fn strlen(s: *const c_char) -> usize;
}

/// Overwrite a C string in place (so a password doesn't linger in memory).
unsafe fn wipe_c(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    let n = strlen(s);
    for i in 0..n {
        std::ptr::write_volatile(s.add(i), 0);
    }
}

/// Overwrite a Rust string's bytes before it's dropped.
pub fn wipe(s: String) {
    let mut v = s.into_bytes();
    for b in v.iter_mut() {
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// Free replies made so far (wiping any password in them) after a failure.
unsafe fn drop_replies(replies: *mut PamResponse, n: isize) {
    for i in 0..n {
        let r = (*replies.offset(i)).resp;
        if !r.is_null() {
            wipe_c(r);
            free(r as *mut c_void);
        }
    }
    free(replies as *mut c_void);
}

/// What the conversation needs: the password, and whether it was given out.
struct ConvData {
    password: *const c_char,
    answered: std::cell::Cell<bool>,
}

/// PAM asks questions. Only the first hidden-input prompt (the password
/// question) gets the password; a second hidden one is asking for something
/// else (a one-time code, another factor), which the lock screen can't
/// answer, so the check fails rather than hand it the password. A visible-input prompt asks for something else (a user
/// name, a one-time code shown on screen...) which the lock screen can't
/// answer, so it fails the check instead of showing the password to it.
/// Messages (errors, information) need no answer. PAM frees the replies with
/// free(), so they must come from malloc.
unsafe extern "C" fn conv(
    n: c_int,
    msgs: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    data: *mut c_void,
) -> c_int {
    if n <= 0 || n > 32 || msgs.is_null() || resp.is_null() {
        return PAM_CONV_ERR;
    }
    let replies = calloc(n as usize, std::mem::size_of::<PamResponse>()) as *mut PamResponse;
    if replies.is_null() {
        return PAM_BUF_ERR;
    }
    if data.is_null() {
        return PAM_CONV_ERR;
    }
    let cd = &*(data as *const ConvData);
    for i in 0..n as isize {
        let m = *msgs.offset(i);
        if m.is_null() {
            drop_replies(replies, i);
            return PAM_CONV_ERR;
        }
        match (*m).msg_style {
            PAM_PROMPT_ECHO_OFF => {
                if cd.answered.replace(true) {
                    eprintln!("rusttouch: the login system asked for a second secret - the lock screen only has the password");
                    drop_replies(replies, i);
                    return PAM_CONV_ERR;
                }
                let copy = strdup(cd.password);
                if copy.is_null() {
                    drop_replies(replies, i);
                    return PAM_BUF_ERR;
                }
                (*replies.offset(i)).resp = copy;
            }
            PAM_PROMPT_ECHO_ON => {
                eprintln!(
                    "rusttouch: the login system asked a question the lock screen can't answer"
                );
                drop_replies(replies, i);
                return PAM_CONV_ERR;
            }
            // PAM_ERROR_MSG, PAM_TEXT_INFO: nothing to answer
            _ => {}
        }
    }
    *resp = replies;
    PAM_SUCCESS
}

/// Blocking (PAM sleeps ~2s on failure) - call from a worker thread. The
/// password is wiped from memory afterwards.
pub fn authenticate(user: &str, password: String) -> bool {
    let (Ok(service), Ok(user)) = (CString::new("rusttouch"), CString::new(user)) else {
        wipe(password);
        return false;
    };
    if password.as_bytes().contains(&0) {
        wipe(password);
        return false;
    }
    let pw = CString::new(password.as_bytes()).expect("checked for NUL above");
    wipe(password);
    let pw = pw.into_raw(); // wiped and freed below
    let data = ConvData {
        password: pw,
        answered: std::cell::Cell::new(false),
    };
    let c = PamConv {
        conv: Some(conv),
        appdata_ptr: &data as *const ConvData as *mut c_void,
    };
    let mut h: *mut c_void = ptr::null_mut();
    let ok = unsafe {
        if pam_start(service.as_ptr(), user.as_ptr(), &c, &mut h) != PAM_SUCCESS {
            false
        } else {
            let rc = pam_authenticate(h, 0);
            pam_end(h, rc);
            rc == PAM_SUCCESS
        }
    };
    unsafe {
        wipe_c(pw);
        drop(CString::from_raw(pw));
    }
    ok
}
