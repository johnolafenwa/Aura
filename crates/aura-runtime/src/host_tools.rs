//! The host commands tests run as child processes.
//!
//! Unix tests name each command by its fixed path. Windows has no such
//! paths: there the tests need a POSIX command set on `PATH`, such as the
//! one Git for Windows installs, and name each command for the search.

/// The POSIX shell.
pub const SH: &str = if cfg!(windows) {
    "sh"
} else {
    concat!("/bin", "/sh")
};
/// Prints its arguments.
pub const ECHO: &str = if cfg!(windows) {
    "echo"
} else {
    concat!("/bin", "/echo")
};
/// Exits with status 0.
pub const TRUE: &str = if cfg!(windows) {
    "true"
} else {
    concat!("/bin", "/true")
};
/// Exits with status 1.
pub const FALSE: &str = if cfg!(windows) {
    "false"
} else {
    concat!("/usr/bin", "/false")
};
