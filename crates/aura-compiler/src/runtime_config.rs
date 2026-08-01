use std::ffi::OsStr;
use std::sync::OnceLock;
use std::thread;

use crate::diag::{Diagnostic, Result};

pub(crate) const BLOCKING_WORKERS_ENV: &str = "AURA_BLOCKING_WORKERS";
pub(crate) const BLOCKING_QUEUE_CAPACITY_ENV: &str = "AURA_BLOCKING_QUEUE_CAPACITY";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BlockingIoPoolConfig {
    pub(crate) worker_count: usize,
    pub(crate) queue_capacity: Option<usize>,
}

static BLOCKING_IO_POOL_CONFIG: OnceLock<Result<BlockingIoPoolConfig>> = OnceLock::new();

fn decode_positive_integer(name: &str, raw: &OsStr) -> Result<usize> {
    let rendered = raw.to_string_lossy();
    let value = raw.to_str().and_then(|text| {
        if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) {
            text.parse::<usize>().ok().filter(|value| *value > 0)
        } else {
            None
        }
    });
    value.ok_or_else(|| {
        Diagnostic::coded(
            "AU4006",
            format!("invalid {name} value `{rendered}`: expected a positive integer"),
        )
    })
}

pub(crate) fn decode_blocking_io_pool_config(
    worker_override: Option<&OsStr>,
    queue_capacity_override: Option<&OsStr>,
    available_workers: Option<usize>,
) -> Result<BlockingIoPoolConfig> {
    let worker_count = match worker_override {
        Some(raw) => decode_positive_integer(BLOCKING_WORKERS_ENV, raw)?,
        None => available_workers.unwrap_or(4).clamp(2, 8),
    };
    let queue_capacity = queue_capacity_override
        .map(|raw| decode_positive_integer(BLOCKING_QUEUE_CAPACITY_ENV, raw))
        .transpose()?;
    Ok(BlockingIoPoolConfig {
        worker_count,
        queue_capacity,
    })
}

fn read_blocking_io_pool_config() -> Result<BlockingIoPoolConfig> {
    decode_blocking_io_pool_config(
        std::env::var_os(BLOCKING_WORKERS_ENV).as_deref(),
        std::env::var_os(BLOCKING_QUEUE_CAPACITY_ENV).as_deref(),
        thread::available_parallelism().ok().map(usize::from),
    )
}

pub(crate) fn blocking_io_pool_config() -> Result<BlockingIoPoolConfig> {
    BLOCKING_IO_POOL_CONFIG
        .get_or_init(read_blocking_io_pool_config)
        .clone()
}

pub(crate) fn validate_runtime_configuration() -> Result<()> {
    blocking_io_pool_config().map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::decode_blocking_io_pool_config;

    #[test]
    fn blocking_io_pool_config_defaults_and_explicit_values_are_exact() {
        let fallback = decode_blocking_io_pool_config(None, None, None).unwrap();
        assert_eq!(fallback.worker_count, 4);
        assert_eq!(fallback.queue_capacity, None);

        for (available, expected) in [(1, 2), (2, 2), (6, 6), (8, 8), (64, 8)] {
            let config = decode_blocking_io_pool_config(None, None, Some(available)).unwrap();
            assert_eq!(config.worker_count, expected);
            assert_eq!(config.queue_capacity, None);
        }

        for workers in ["1", "7", "64"] {
            let config = decode_blocking_io_pool_config(
                Some(OsStr::new(workers)),
                Some(OsStr::new("1")),
                Some(3),
            )
            .unwrap();
            assert_eq!(config.worker_count, workers.parse::<usize>().unwrap());
            assert_eq!(config.queue_capacity, Some(1));
        }
    }

    #[test]
    fn blocking_io_pool_config_rejects_every_invalid_value_class_for_each_setting() {
        let overflow = (usize::MAX as u128 + 1).to_string();
        for (name, workers, capacity) in [
            ("AURA_BLOCKING_WORKERS", Some(""), None),
            ("AURA_BLOCKING_WORKERS", Some("0"), None),
            ("AURA_BLOCKING_WORKERS", Some("+1"), None),
            ("AURA_BLOCKING_WORKERS", Some("-1"), None),
            ("AURA_BLOCKING_WORKERS", Some(" 1"), None),
            ("AURA_BLOCKING_WORKERS", Some("1 "), None),
            ("AURA_BLOCKING_WORKERS", Some("1.0"), None),
            ("AURA_BLOCKING_WORKERS", Some("١"), None),
            ("AURA_BLOCKING_QUEUE_CAPACITY", None, Some("")),
            ("AURA_BLOCKING_QUEUE_CAPACITY", None, Some("0")),
            ("AURA_BLOCKING_QUEUE_CAPACITY", None, Some("+1")),
            ("AURA_BLOCKING_QUEUE_CAPACITY", None, Some("-1")),
            ("AURA_BLOCKING_QUEUE_CAPACITY", None, Some(" 1")),
            ("AURA_BLOCKING_QUEUE_CAPACITY", None, Some("1 ")),
            ("AURA_BLOCKING_QUEUE_CAPACITY", None, Some("1.0")),
            ("AURA_BLOCKING_QUEUE_CAPACITY", None, Some("١")),
        ] {
            let error = decode_blocking_io_pool_config(
                workers.map(OsStr::new),
                capacity.map(OsStr::new),
                Some(4),
            )
            .expect_err("the present value must be rejected");
            assert_eq!(error.code, "AU4006");
            assert!(
                error
                    .message
                    .starts_with(&format!("invalid {name} value `")),
                "{}",
                error.message
            );
            assert!(
                error.message.ends_with("`: expected a positive integer"),
                "{}",
                error.message
            );
        }

        for (name, workers, capacity) in [
            ("AURA_BLOCKING_WORKERS", Some(overflow.as_str()), None),
            (
                "AURA_BLOCKING_QUEUE_CAPACITY",
                None,
                Some(overflow.as_str()),
            ),
        ] {
            let error = decode_blocking_io_pool_config(
                workers.map(OsStr::new),
                capacity.map(OsStr::new),
                Some(4),
            )
            .expect_err("overflow must be rejected");
            assert_eq!(error.code, "AU4006");
            assert_eq!(
                error.message,
                format!("invalid {name} value `{overflow}`: expected a positive integer")
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn blocking_io_pool_config_rejects_non_unicode_values_for_each_setting() {
        use std::os::unix::ffi::OsStrExt;

        let invalid = OsStr::from_bytes(b"invalid-\xff");
        for (name, workers, capacity) in [
            ("AURA_BLOCKING_WORKERS", Some(invalid), None),
            ("AURA_BLOCKING_QUEUE_CAPACITY", None, Some(invalid)),
        ] {
            let error = decode_blocking_io_pool_config(workers, capacity, Some(4))
                .expect_err("non-Unicode values must be rejected");
            assert_eq!(error.code, "AU4006");
            assert_eq!(
                error.message,
                format!("invalid {name} value `invalid-\u{fffd}`: expected a positive integer")
            );
        }
    }
}
