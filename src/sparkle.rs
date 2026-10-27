//! Sparkle lives in the packaged `.app`. A `cargo run` binary has no
//! framework beside it, so these calls become a no-op there.

#[cfg(target_os = "macos")]
mod sys {
    unsafe extern "C" {
        pub fn folio_sparkle_start();
        pub fn folio_sparkle_available() -> i32;
        pub fn folio_sparkle_check();
        pub fn folio_sparkle_check_information();
        pub fn folio_sparkle_check_state() -> i32;
        pub fn folio_sparkle_found_version() -> *const std::ffi::c_char;
        pub fn folio_sparkle_found_build() -> *const std::ffi::c_char;
    }
}

/// An update the feed advertises that is newer than the running build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub version: String,
    pub build: u64,
}

/// Start the updater so scheduled checks from Info.plist can run.
pub fn start() {
    #[cfg(target_os = "macos")]
    unsafe {
        sys::folio_sparkle_start();
    }
}

/// Open Sparkle's update window. `false` means this build has no framework.
pub fn check() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        if sys::folio_sparkle_available() != 0 {
            sys::folio_sparkle_check();
            return true;
        }
    }
    false
}

/// Ask the feed in the background, without any Sparkle UI. Returns `false`
/// when there is no framework to ask; otherwise poll [`background_result`].
pub fn begin_background_check() -> bool {
    #[cfg(target_os = "macos")]
    unsafe {
        if sys::folio_sparkle_available() != 0 {
            sys::folio_sparkle_check_information();
            return true;
        }
    }
    false
}

/// `None` while the check is still running; `Some(None)` when this build is
/// current; `Some(Some(found))` when the feed has something newer.
pub fn background_result() -> Option<Option<Found>> {
    #[cfg(target_os = "macos")]
    unsafe {
        match sys::folio_sparkle_check_state() {
            0 => None,
            2 => {
                let version = c_string(sys::folio_sparkle_found_version());
                let build = c_string(sys::folio_sparkle_found_build())
                    .parse()
                    .unwrap_or(0);
                Some(Some(Found { version, build }))
            }
            _ => Some(None),
        }
    }
    #[cfg(not(target_os = "macos"))]
    Some(None)
}

#[cfg(target_os = "macos")]
unsafe fn c_string(ptr: *const std::ffi::c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn unpackaged_builds_have_no_sparkle() {
        assert!(!super::check());
        assert!(!super::begin_background_check());
    }
}
