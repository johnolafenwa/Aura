//! Atomic operations and orderings shared by checking and both execution paths.
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomicOperation {
    Load,
    Store,
    Add,
    Swap,
    CompareExchange,
    Fence,
}

impl AtomicOperation {
    pub fn of(name: &str) -> Option<Self> {
        match name.strip_prefix("mem::").unwrap_or(name).split("_uint").next()? {
            "atomic_load" => Some(Self::Load),
            "atomic_store" => Some(Self::Store),
            "atomic_add" => Some(Self::Add),
            "atomic_swap" => Some(Self::Swap),
            "atomic_compare_exchange" => Some(Self::CompareExchange),
            "fence" => Some(Self::Fence),
            _ => None,
        }
    }

    pub fn arity(self) -> usize {
        match self {
            Self::Fence => 0,
            Self::Load => 1,
            Self::Store | Self::Swap | Self::Add => 2,
            Self::CompareExchange => 3,
        }
    }

    pub fn orders(self, success: Option<&str>, failure: Option<&str>) -> Result<(Ordering, Ordering), &'static str> {
        use Ordering::*;
        let success = parse(success.unwrap_or("seq_cst"))?;
        match (self, success) {
            (Self::Load, Release | AcqRel) => return Err("a load cannot release"),
            (Self::Store, Acquire | AcqRel) => return Err("a store cannot acquire"),
            (Self::Fence, Relaxed) => return Err("a fence requires acquire or release ordering"),
            _ => {}
        }
        let failure = match failure {
            None => match success {
                Release => Relaxed,
                AcqRel => Acquire,
                order => order,
            },
            Some(name) => {
                let failure = parse(name)?;
                if self != Self::CompareExchange || !matches!((failure, success),
                    (Relaxed, _) | (Acquire, Acquire | AcqRel | SeqCst) | (SeqCst, SeqCst)) {
                    return Err("failure ordering cannot release or exceed the success ordering");
                }
                failure
            }
        };
        Ok((success, failure))
    }
}

fn parse(name: &str) -> Result<Ordering, &'static str> {
    match name {
        "relaxed" => Ok(Ordering::Relaxed),
        "acquire" => Ok(Ordering::Acquire),
        "release" => Ok(Ordering::Release),
        "acq_rel" => Ok(Ordering::AcqRel),
        "seq_cst" => Ok(Ordering::SeqCst),
        _ => Err("expected relaxed, acquire, release, acq_rel or seq_cst"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering::*;
    #[test]
    fn order_pairs_follow_the_atomic_operation() {
        use AtomicOperation::*;
        assert_eq!(Load.orders(None, None), Ok((SeqCst, SeqCst)));
        for (op, name) in [(Load, "release"), (Store, "acq_rel"), (Fence, "relaxed"), (Add, "unknown")] {
            assert!(op.orders(Some(name), None).is_err());
        }
        assert_eq!(CompareExchange.orders(Some("release"), None), Ok((Release, Relaxed)));
        assert_eq!(CompareExchange.orders(Some("acq_rel"), None), Ok((AcqRel, Acquire)));
        for failure in ["release", "acq_rel", "seq_cst", "bad"] {
            assert!(CompareExchange.orders(Some("acquire"), Some(failure)).is_err());
        }
        assert!(CompareExchange.orders(Some("release"), Some("acquire")).is_err());
        assert!(CompareExchange.orders(Some("acq_rel"), Some("relaxed")).is_ok());
        assert!(Swap.orders(None, Some("relaxed")).is_err());
    }
}
