use anyhow::{Context, Result, bail};
use std::{
    io::Write,
    process::{Command, Stdio},
};

pub fn copy_text(text: &str) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("Clipboard insertion is supported on macOS.");
    }
    let mut process = Command::new("/usr/bin/pbcopy")
        .stdin(Stdio::piped())
        .spawn()?;
    process
        .stdin
        .take()
        .context("Cannot open clipboard")?
        .write_all(text.as_bytes())?;
    if !process.wait()?.success() {
        bail!("Could not copy text to the clipboard.");
    }
    Ok(())
}
#[cfg(target_os = "macos")]
pub fn frontmost_pid() -> Option<i32> {
    objc2_app_kit::NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier())
}
#[cfg(not(target_os = "macos"))]
pub fn frontmost_pid() -> Option<i32> {
    None
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn CGEventCreateKeyboardEvent(
        source: *const std::ffi::c_void,
        key: u16,
        down: bool,
    ) -> *mut std::ffi::c_void;
    fn CGEventSetFlags(event: *mut std::ffi::c_void, flags: u64);
    fn CGEventPostToPid(pid: i32, event: *mut std::ffi::c_void);
}
#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: *const std::ffi::c_void);
}

pub fn accessibility_allowed() -> bool {
    #[cfg(target_os = "macos")]
    // SAFETY: this read-only system API has no preconditions.
    unsafe {
        AXIsProcessTrusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
pub fn paste_to(pid: i32) -> Result<()> {
    if !accessibility_allowed() {
        bail!(
            "Text copied. To insert it automatically, allow FastAF Flow in System Settings → Privacy & Security → Accessibility."
        );
    }
    if frontmost_pid() != Some(pid) {
        bail!(
            "Text copied. The active app changed while transcribing; press ⌘V where you want the text."
        );
    }
    #[cfg(target_os = "macos")]
    // SAFETY: events are checked for null, posted to the captured target PID,
    // and released exactly once. 0x09 is the macOS virtual key code for V.
    unsafe {
        let down = CGEventCreateKeyboardEvent(std::ptr::null(), 0x09, true);
        let up = CGEventCreateKeyboardEvent(std::ptr::null(), 0x09, false);
        if down.is_null() || up.is_null() {
            if !down.is_null() {
                CFRelease(down);
            }
            if !up.is_null() {
                CFRelease(up);
            }
            bail!("Text copied, but macOS could not create a paste event.");
        }
        CGEventSetFlags(down, 1 << 20);
        CGEventSetFlags(up, 1 << 20);
        CGEventPostToPid(pid, down);
        CGEventPostToPid(pid, up);
        CFRelease(down);
        CFRelease(up);
    }
    Ok(())
}
pub fn open_privacy(microphone: bool) {
    let page = if microphone {
        "Privacy_Microphone"
    } else {
        "Privacy_Accessibility"
    };
    let _ = Command::new("/usr/bin/open")
        .arg(format!(
            "x-apple.systempreferences:com.apple.preference.security?{page}"
        ))
        .spawn();
}
