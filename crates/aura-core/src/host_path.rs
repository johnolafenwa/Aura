//! Paths as the host spells them.

use std::io;
use std::path::{Path, PathBuf};

/// `std::fs::canonicalize`, with the result in the form people and other
/// programs write.
///
/// Windows answers with a verbatim path such as `\\?\C:\work\main.au`.
/// Git, the Microsoft linker and many other programs reject that form, and
/// it is not what a user typed, so the prefix is dropped where the plain
/// path names the same file. Other hosts return the path unchanged.
pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    if cfg!(windows) {
        if let Some(plain) = canonical.to_str().and_then(without_verbatim_prefix) {
            return Ok(PathBuf::from(plain));
        }
    }
    Ok(canonical)
}

/// The plain spelling of a Windows verbatim path, or `None` when `path` is
/// not verbatim or has no plain spelling.
///
/// `\\?\C:\dir` becomes `C:\dir` and `\\?\UNC\server\share` becomes
/// `\\server\share`. A plain path is limited to 260 characters and gives
/// some names a special meaning, so a longer path, and one with a
/// component that ends in a dot or a space, stay verbatim.
pub fn without_verbatim_prefix(path: &str) -> Option<String> {
    let rest = path.strip_prefix(r"\\?\")?;
    let plain = if let Some(share) = rest.strip_prefix(r"UNC\") {
        format!(r"\\{share}")
    } else {
        let bytes = rest.as_bytes();
        let has_drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\';
        if !has_drive {
            return None;
        }
        rest.to_string()
    };
    let fits = plain.len() < 260;
    let plain_components = plain
        .split('\\')
        .all(|component| !component.ends_with('.') && !component.ends_with(' '));
    (fits && plain_components && !plain.contains('/')).then_some(plain)
}

#[cfg(test)]
mod tests {
    use super::{canonicalize, without_verbatim_prefix};

    #[test]
    fn verbatim_drive_and_share_paths_lose_their_prefix() {
        assert_eq!(
            without_verbatim_prefix(r"\\?\C:\work\main.au").as_deref(),
            Some(r"C:\work\main.au")
        );
        assert_eq!(
            without_verbatim_prefix(r"\\?\UNC\server\share\main.au").as_deref(),
            Some(r"\\server\share\main.au")
        );
    }

    #[test]
    fn paths_with_no_plain_spelling_stay_as_they_are() {
        assert_eq!(without_verbatim_prefix(r"C:\work\main.au"), None);
        assert_eq!(without_verbatim_prefix("/home/work/main.au"), None);
        assert_eq!(without_verbatim_prefix(r"\\?\Volume{1234}\main.au"), None);
        assert_eq!(without_verbatim_prefix(r"\\?\C:\work\name.\main.au"), None);
        assert_eq!(without_verbatim_prefix(r"\\?\C:\work\name \main.au"), None);
        let long = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(without_verbatim_prefix(&long), None);
    }

    #[test]
    fn canonicalize_resolves_an_existing_path_and_reports_a_missing_one() {
        let directory = std::env::temp_dir();
        let canonical = canonicalize(&directory).expect("the temporary directory exists");
        assert!(canonical.is_absolute());
        assert!(!canonical.to_string_lossy().starts_with(r"\\?\"));
        assert!(canonicalize(directory.join("aura-host-path-missing-entry")).is_err());
    }
}
