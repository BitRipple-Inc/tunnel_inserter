use std::os::unix::io::{RawFd};
use nix::fcntl::{fcntl, FcntlArg, FdFlag};

/// Set or clear the FD_CLOEXEC flag on a file descriptor
pub fn set_cloexec(fd: RawFd, enable: bool) {
    let flags = fcntl(fd, FcntlArg::F_GETFD).expect("fcntl failed"); // Get current flags
    let new_flags = if enable {
        FdFlag::from_bits_truncate(flags) | FdFlag::FD_CLOEXEC // Set the CLOEXEC flag without affecting other ones
    } else {
        FdFlag::from_bits_truncate(flags) & !FdFlag::FD_CLOEXEC // Clear CLOEXEC flag preserving the other ones
    };
    fcntl(fd, FcntlArg::F_SETFD(new_flags)).expect("Failed to set FD_CLOEXEC"); // Set modified flags
}
