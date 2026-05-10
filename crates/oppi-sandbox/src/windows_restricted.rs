#![cfg(windows)]

//! Restricted-token helpers for OPPi's Windows sandbox adapter.
//!
//! These primitives intentionally stay small and OPPi-owned: they create a
//! restricted primary token and lower its mandatory integrity label. Process
//! launch and per-root writable ACL/capability work can build on this without
//! changing the public planning API.

use std::ffi::c_void;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows_sys::Win32::Security::{
    CreateRestrictedToken, CreateWellKnownSid, DISABLE_MAX_PRIVILEGE, LOGON32_LOGON_BATCH,
    LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, LUA_TOKEN, LogonUserW, SID_AND_ATTRIBUTES,
    SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_ADJUST_PRIVILEGES, TOKEN_ADJUST_SESSIONID,
    TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenIntegrityLevel,
    WRITE_RESTRICTED, WinLowLabelSid,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const SE_GROUP_INTEGRITY: u32 = 0x0000_0020;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsRestrictedTokenStatus {
    pub available: bool,
    pub low_integrity_supported: bool,
    pub message: String,
}

#[derive(Debug)]
pub struct WindowsRestrictedToken {
    handle: HANDLE,
}

impl WindowsRestrictedToken {
    pub fn as_handle(&self) -> HANDLE {
        self.handle
    }

    pub fn into_raw_handle(mut self) -> HANDLE {
        let handle = self.handle;
        self.handle = null_mut();
        handle
    }
}

impl Drop for WindowsRestrictedToken {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                CloseHandle(self.handle);
            }
        }
    }
}

pub fn windows_restricted_token_status() -> WindowsRestrictedTokenStatus {
    match create_restricted_low_integrity_primary_token() {
        Ok(_) => WindowsRestrictedTokenStatus {
            available: true,
            low_integrity_supported: true,
            message: "restricted primary token with low integrity can be created".to_string(),
        },
        Err(error) => WindowsRestrictedTokenStatus {
            available: false,
            low_integrity_supported: false,
            message: format!("restricted low-integrity token is unavailable: {error}"),
        },
    }
}

pub fn create_restricted_low_integrity_primary_token() -> Result<WindowsRestrictedToken, String> {
    unsafe {
        let base = open_current_process_token()?;
        let result = create_restricted_low_integrity_primary_token_from(base);
        CloseHandle(base);
        result
    }
}

pub fn create_restricted_low_integrity_primary_token_for_credentials(
    username: &str,
    domain: Option<&str>,
    password: &str,
) -> Result<WindowsRestrictedToken, String> {
    if username.trim().is_empty() {
        return Err("Windows sandbox username is required".to_string());
    }
    unsafe {
        let base = logon_user_token(username, domain, password)?;
        let result = create_restricted_low_integrity_primary_token_from(base);
        CloseHandle(base);
        result
    }
}

/// Create a restricted primary token from an existing primary token and set its
/// mandatory integrity level to Low.
///
/// The returned token is suitable for `CreateProcessAsUserW`/`CreateProcessWithTokenW`
/// in the next adapter layer. It does not by itself grant per-workspace write
/// access; that still requires the Windows writable-root strategy tracked in
/// Plan 20.
///
/// # Safety
///
/// `base_token` must be a valid, open Windows primary-token handle with the
/// duplicate/query/assign/default-adjust privileges requested by this module.
/// The caller remains responsible for closing `base_token`; this function only
/// owns and closes the restricted token it creates.
pub unsafe fn create_restricted_low_integrity_primary_token_from(
    base_token: HANDLE,
) -> Result<WindowsRestrictedToken, String> {
    let mut restricted: HANDLE = null_mut();
    let flags = DISABLE_MAX_PRIVILEGE | LUA_TOKEN | WRITE_RESTRICTED;
    let created = unsafe {
        CreateRestrictedToken(
            base_token,
            flags,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            &mut restricted,
        )
    };
    if created == 0 || restricted.is_null() {
        return Err(format_last_error("CreateRestrictedToken"));
    }

    let token = WindowsRestrictedToken { handle: restricted };
    unsafe { set_low_integrity(token.handle) }?;
    Ok(token)
}

unsafe fn logon_user_token(
    username: &str,
    domain: Option<&str>,
    password: &str,
) -> Result<HANDLE, String> {
    let username = to_wide(username);
    let domain = domain.map(to_wide).unwrap_or_else(|| vec![0]);
    let password = to_wide(password);
    let mut token: HANDLE = null_mut();
    let mut logged_on = unsafe {
        LogonUserW(
            username.as_ptr(),
            domain.as_ptr(),
            password.as_ptr(),
            LOGON32_LOGON_BATCH,
            LOGON32_PROVIDER_DEFAULT,
            &mut token,
        )
    };
    if logged_on == 0 {
        logged_on = unsafe {
            LogonUserW(
                username.as_ptr(),
                domain.as_ptr(),
                password.as_ptr(),
                LOGON32_LOGON_INTERACTIVE,
                LOGON32_PROVIDER_DEFAULT,
                &mut token,
            )
        };
    }
    if logged_on == 0 || token.is_null() {
        return Err(format_last_error("LogonUserW"));
    }
    Ok(token)
}

unsafe fn open_current_process_token() -> Result<HANDLE, String> {
    let desired = TOKEN_DUPLICATE
        | TOKEN_QUERY
        | TOKEN_ASSIGN_PRIMARY
        | TOKEN_ADJUST_DEFAULT
        | TOKEN_ADJUST_SESSIONID
        | TOKEN_ADJUST_PRIVILEGES;
    let mut token: HANDLE = null_mut();
    let opened = unsafe { OpenProcessToken(GetCurrentProcess(), desired, &mut token) };
    if opened == 0 || token.is_null() {
        return Err(format_last_error("OpenProcessToken"));
    }
    Ok(token)
}

unsafe fn set_low_integrity(token: HANDLE) -> Result<(), String> {
    let mut sid_size = 0;
    unsafe {
        CreateWellKnownSid(WinLowLabelSid, null_mut(), null_mut(), &mut sid_size);
    }
    if sid_size == 0 {
        return Err(format_last_error("CreateWellKnownSid(size)"));
    }

    let mut sid = vec![0_u8; sid_size as usize];
    let created = unsafe {
        CreateWellKnownSid(
            WinLowLabelSid,
            null_mut(),
            sid.as_mut_ptr() as *mut c_void,
            &mut sid_size,
        )
    };
    if created == 0 {
        return Err(format_last_error("CreateWellKnownSid(WinLowLabelSid)"));
    }

    let mut label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid.as_mut_ptr() as *mut c_void,
            Attributes: SE_GROUP_INTEGRITY,
        },
    };
    let size = std::mem::size_of::<TOKEN_MANDATORY_LABEL>() as u32 + sid_size;
    let set = unsafe {
        SetTokenInformation(
            token,
            TokenIntegrityLevel,
            &mut label as *mut _ as *mut c_void,
            size,
        )
    };
    if set == 0 {
        return Err(format_last_error(
            "SetTokenInformation(TokenIntegrityLevel)",
        ));
    }
    Ok(())
}

fn format_last_error(operation: &str) -> String {
    let error = unsafe { GetLastError() };
    format!("{operation} failed: {error}")
}

fn to_wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restricted_token_status_message_is_stable() {
        let status = windows_restricted_token_status();
        assert!(
            status.message.contains("restricted") || status.message.contains("unavailable"),
            "{status:?}"
        );
    }
}
