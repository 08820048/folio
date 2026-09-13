//! Sparkle lives in the packaged `.app`. A `cargo run` binary has no
//! framework beside it, so these calls become a no-op there.

#[cfg(target_os = "macos")]
mod sys {
    unsafe extern "C" {
        pub fn folio_sparkle_start();
        pub fn folio_sparkle_available() -> i32;
        pub fn folio_sparkle_check();
    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn unpackaged_builds_have_no_sparkle() {
        assert!(!super::check());
    }
}
