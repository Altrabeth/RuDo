//! The operating systems RuDo distinguishes.
//!
//! Every function whose result differs between operating systems takes a [`Platform`] as a
//! parameter, so that the behaviour of each one is unit tested on every machine. Only thin
//! wrappers in the imperative shell call [`Platform::current`].

/// An operating system family with its own conventions for folders and programs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum Platform {
    /// Linux.
    Linux,
    /// macOS, which RuDo treats like Linux: it follows the XDG conventions there too.
    MacOs,
    /// Windows.
    Windows,
    /// Any other Unix system, such as FreeBSD, treated like Linux.
    OtherUnix,
}

impl Platform {
    /// Returns the platform RuDo was compiled for.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::OtherUnix
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn current_platform_is_linux_on_linux() {
        let system_under_test = Platform::current();

        assert_eq!(system_under_test, Platform::Linux);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn current_platform_is_macos_on_macos() {
        let system_under_test = Platform::current();

        assert_eq!(system_under_test, Platform::MacOs);
    }

    #[test]
    #[cfg(windows)]
    fn current_platform_is_windows_on_windows() {
        let system_under_test = Platform::current();

        assert_eq!(system_under_test, Platform::Windows);
    }
}
