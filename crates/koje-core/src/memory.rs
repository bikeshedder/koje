//! Memory limits per context: classify current usage against soft and hard limits.
//!
//! - Soft limit: warn at 90 % and at 100 %, keep running.
//! - Hard limit: orderly shutdown (save to `default`, then `down`).
//! - The kernel's `memory.max` sits slightly above the hard limit as a safety net.

use crate::config::Limits;
use crate::size::ByteSize;

/// Percentage of the soft limit at which the first warning fires.
pub const WARN_PERCENT: u64 = 90;

/// Head-room between the hard limit and the cgroup `memory.max` safety net.
pub const KERNEL_HEADROOM_PERCENT: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryLevel {
    Normal,
    /// At or above 90 % of the soft limit.
    NearSoft,
    /// At or above the soft limit.
    OverSoft,
    /// At or above the hard limit → shut down.
    OverHard,
}

pub fn classify(usage: ByteSize, limits: &Limits) -> MemoryLevel {
    let used = u128::from(usage.0);
    if let Some(hard) = limits.memory_hard
        && used >= u128::from(hard.0)
    {
        return MemoryLevel::OverHard;
    }
    if let Some(soft) = limits.memory_soft {
        let soft = u128::from(soft.0);
        if used >= soft {
            return MemoryLevel::OverSoft;
        }
        if used * 100 >= soft * u128::from(WARN_PERCENT) {
            return MemoryLevel::NearSoft;
        }
    }
    MemoryLevel::Normal
}

/// Value for the cgroup `memory.max` safety net, or `None` without a hard limit.
pub fn kernel_limit(limits: &Limits) -> Option<ByteSize> {
    limits
        .memory_hard
        .map(|h| ByteSize(h.0.saturating_add(h.0 / 100 * KERNEL_HEADROOM_PERCENT)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> Limits {
        Limits {
            memory_soft: Some(ByteSize(6 << 30)),
            memory_hard: Some(ByteSize(8 << 30)),
        }
    }

    fn gib_tenths(t: u64) -> ByteSize {
        ByteSize((1 << 30) / 10 * t)
    }

    #[test]
    fn classifies_levels() {
        let l = limits();
        assert_eq!(classify(gib_tenths(50), &l), MemoryLevel::Normal);
        assert_eq!(classify(gib_tenths(55), &l), MemoryLevel::NearSoft); // 5.5 / 6 = 92 %
        assert_eq!(classify(gib_tenths(61), &l), MemoryLevel::OverSoft);
        assert_eq!(classify(ByteSize(8 << 30), &l), MemoryLevel::OverHard);
    }

    #[test]
    fn no_limits_is_always_normal() {
        assert_eq!(
            classify(ByteSize(u64::MAX), &Limits::default()),
            MemoryLevel::Normal
        );
    }

    #[test]
    fn kernel_limit_has_headroom() {
        let k = kernel_limit(&limits()).unwrap();
        assert!(k.0 > 8 << 30);
        assert_eq!(kernel_limit(&Limits::default()), None);
    }
}
