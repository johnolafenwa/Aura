//! The proof cache of `aura verify`.
//!
//! An entry says which checks of a function were proved, and a build with
//! `--remove-proved-checks` drops the run-time checks it names, so an
//! entry is trusted only when this user's verifier wrote it:
//!
//! - the directory is a real directory, owned by the current user, that no
//!   one else can write to;
//! - each entry carries its cache key and a keyed hash (HMAC-SHA-256) of
//!   the key and the statuses, made with a secret only the user can read;
//! - entries are written to a fresh private file and renamed into place,
//!   and neither reads nor writes follow a symlink.
//!
//! An entry that fails any of these is solved again. A cache that cannot be
//! used only costs the time to solve.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::Status;

/// The name of the secret the entries are signed with.
const SECRET_NAME: &str = ".aura-verify-secret";
const SECRET_LEN: usize = 32;
/// An entry larger than this is not read.
const MAX_ENTRY_BYTES: u64 = 16 * 1024 * 1024;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) struct ProofCache {
    dir: PathBuf,
    secret: [u8; SECRET_LEN],
}

#[derive(Serialize, Deserialize)]
struct Entry {
    key: String,
    statuses: Vec<Status>,
    mac: String,
}

impl ProofCache {
    /// The cache at `dir`, creating it, or `None` when it cannot be used
    /// safely.
    pub(super) fn open(dir: &Path) -> Option<Self> {
        create_private_dir(dir)?;
        let secret = read_or_create_secret(&dir.join(SECRET_NAME))?;
        Some(Self {
            dir: dir.to_path_buf(),
            secret,
        })
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    /// The statuses cached for `key`, when there is an entry this user's
    /// verifier wrote for it with one status for each of `obligations`.
    pub(super) fn read(&self, key: &str, obligations: usize) -> Option<Vec<Status>> {
        let mut file = open_nofollow(&self.entry_path(key))?;
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() || metadata.len() > MAX_ENTRY_BYTES {
            return None;
        }
        let mut text = String::new();
        file.read_to_string(&mut text).ok()?;
        let entry: Entry = serde_json::from_str(&text).ok()?;
        if entry.key != key || entry.statuses.len() != obligations {
            return None;
        }
        let expected = self.mac(key, &entry.statuses)?;
        constant_time_eq(expected.as_bytes(), entry.mac.as_bytes()).then_some(entry.statuses)
    }

    /// Records decided statuses for `key`. Failing to write only loses the
    /// entry.
    pub(super) fn write(&self, key: &str, statuses: &[Status]) {
        let Some(mac) = self.mac(key, statuses) else {
            return;
        };
        let entry = Entry {
            key: key.to_string(),
            statuses: statuses.to_vec(),
            mac,
        };
        let Ok(text) = serde_json::to_string(&entry) else {
            return;
        };
        let temp = self.dir.join(format!(
            ".{key}.{}.{}.tmp",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let written = create_new_private(&temp)
            .and_then(|mut file| file.write_all(text.as_bytes()))
            .and_then(|()| fs::rename(&temp, self.entry_path(key)));
        if written.is_err() {
            let _ = fs::remove_file(&temp);
        }
    }

    fn mac(&self, key: &str, statuses: &[Status]) -> Option<String> {
        let statuses = serde_json::to_string(statuses).ok()?;
        let mut message = Vec::with_capacity(key.len() + statuses.len() + 1);
        message.extend_from_slice(key.as_bytes());
        message.push(b'\n');
        message.extend_from_slice(statuses.as_bytes());
        Some(hex(&hmac_sha256(&self.secret, &message)))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// HMAC-SHA-256 (RFC 2104) with a key no longer than the block.
pub(super) fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    debug_assert!(key.len() <= 64);
    let mut block = [0u8; 64];
    block[..key.len()].copy_from_slice(key);
    let pad = |byte: u8| block.map(|k| k ^ byte);
    let mut inner = Sha256::new();
    inner.update(pad(0x36));
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(pad(0x5c));
    outer.update(inner);
    outer.finalize().into()
}

/// Creates `dir` and its missing parents private to the user, and checks
/// that it is a real directory no one else can write to.
fn create_private_dir(dir: &Path) -> Option<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir).ok()?;
    let metadata = fs::symlink_metadata(dir).ok()?;
    if !metadata.file_type().is_dir() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // SAFETY: geteuid has no preconditions.
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o022 != 0
        {
            return None;
        }
    }
    Some(())
}

fn read_or_create_secret(path: &Path) -> Option<[u8; SECRET_LEN]> {
    if let Some(secret) = read_secret(path) {
        return Some(secret);
    }
    let secret = random_secret()?;
    match create_new_private(path) {
        Ok(mut file) => {
            if file.write_all(&secret).is_err() {
                drop(file);
                let _ = fs::remove_file(path);
                return None;
            }
            Some(secret)
        }
        // Another verifier made it first: use that one.
        Err(_) => read_secret(path),
    }
}

fn read_secret(path: &Path) -> Option<[u8; SECRET_LEN]> {
    let mut file = open_nofollow(path)?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() != SECRET_LEN as u64 {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        // SAFETY: geteuid has no preconditions.
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return None;
        }
    }
    let mut secret = [0u8; SECRET_LEN];
    file.read_exact(&mut secret).ok()?;
    Some(secret)
}

#[cfg(unix)]
fn random_secret() -> Option<[u8; SECRET_LEN]> {
    let mut secret = [0u8; SECRET_LEN];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut secret))
        .ok()?;
    Some(secret)
}

/// The standard library seeds each `RandomState` from the system's random
/// source; hashing with several of them gives the secret's bytes.
#[cfg(not(unix))]
fn random_secret() -> Option<[u8; SECRET_LEN]> {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = Sha256::new();
    for round in 0u64..8 {
        let mut state = std::collections::hash_map::RandomState::new().build_hasher();
        state.write_u64(round);
        hasher.update(state.finish().to_le_bytes());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    Some(hasher.finalize().into())
}

fn open_nofollow(path: &Path) -> Option<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    if fs::symlink_metadata(path).ok()?.file_type().is_symlink() {
        return None;
    }
    options.open(path).ok()
}

fn create_new_private(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}
