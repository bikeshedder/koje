//! Memory limits per context (ADR 0035): usage is the working set; classify it
//! against soft and hard limits and decide when to notify.
//!
//! - Soft limit: warn at 90 % and at 100 %, keep running; again only after
//!   usage fell below 85 %.
//! - Hard limit: orderly shutdown (save into the current state, then `down`).
//! - The kernel's `memory.max` sits slightly above the hard limit as a safety net.

use crate::config::Limits;
use crate::size::ByteSize;

/// Percentage of the soft limit at which the first warning fires.
pub const WARN_PERCENT: u64 = 90;

/// Head-room between the hard limit and the cgroup `memory.max` safety net.
pub const KERNEL_HEADROOM_PERCENT: u64 = 10;

/// Below this share of the soft limit, warnings may come again.
pub const REARM_PERCENT: u64 = 85;

/// A context's memory as its cgroup reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoryUsage {
    /// `memory.current`: everything charged, page cache included.
    pub current: ByteSize,
    /// `anon` from `memory.stat`: heaps, stacks, tmpfs.
    pub anon: ByteSize,
    /// `inactive_file` from `memory.stat`: cache the kernel drops first.
    pub inactive_file: ByteSize,
}

impl MemoryUsage {
    /// What limits look at (ADR 0035): `memory.current` minus inactive file cache.
    pub fn working_set(&self) -> ByteSize {
        ByteSize(self.current.0.saturating_sub(self.inactive_file.0))
    }
}

/// `anon` and `inactive_file` from a cgroup's `memory.stat`.
pub fn parse_stat(text: &str) -> (ByteSize, ByteSize) {
    let field = |name: &str| {
        text.lines()
            .find_map(|l| {
                l.strip_prefix(name)?
                    .strip_prefix(' ')?
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
            .map_or(ByteSize(0), ByteSize)
    };
    (field("anon"), field("inactive_file"))
}

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

/// Which levels a context was notified about (ADR 0035): once per crossing,
/// again after usage fell below [`REARM_PERCENT`] of the soft limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Watch {
    notified: MemoryLevel,
}

impl Default for Watch {
    fn default() -> Self {
        Self {
            notified: MemoryLevel::Normal,
        }
    }
}

impl Watch {
    /// The level to act on now (notify; shut down at `OverHard`), if any.
    pub fn step(&mut self, usage: ByteSize, limits: &Limits) -> Option<MemoryLevel> {
        let level = classify(usage, limits);
        let rearm = limits.memory_soft.is_none_or(|soft| {
            u128::from(usage.0) * 100 < u128::from(soft.0) * u128::from(REARM_PERCENT)
        });
        if level == MemoryLevel::Normal && rearm {
            self.notified = MemoryLevel::Normal;
            return None;
        }
        if level > self.notified {
            self.notified = level;
            return Some(level);
        }
        None
    }
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
    fn working_set_from_the_cgroup() {
        let stat = "anon 734003200\nfile 1073741824\nkernel 52428800\ninactive_file 805306368\nactive_file 268435456\ninactive_anon 0\n";
        let (anon, inactive) = parse_stat(stat);
        assert_eq!(
            (anon, inactive),
            (ByteSize(734_003_200), ByteSize(805_306_368))
        );
        let u = MemoryUsage {
            current: ByteSize(1_900_000_000),
            anon,
            inactive_file: inactive,
        };
        assert_eq!(u.working_set(), ByteSize(1_900_000_000 - 805_306_368));
        assert_eq!(parse_stat(""), (ByteSize(0), ByteSize(0)));
        let odd = MemoryUsage {
            current: ByteSize(1),
            anon: ByteSize(0),
            inactive_file: ByteSize(5),
        };
        assert_eq!(odd.working_set(), ByteSize(0), "never negative");
    }

    #[test]
    fn notifies_once_per_crossing() {
        let l = limits(); // soft 6 GiB, hard 8 GiB
        let mut w = Watch::default();
        assert_eq!(w.step(gib_tenths(50), &l), None);
        assert_eq!(w.step(gib_tenths(55), &l), Some(MemoryLevel::NearSoft));
        assert_eq!(w.step(gib_tenths(56), &l), None, "no spam");
        assert_eq!(
            w.step(gib_tenths(53), &l),
            None,
            "below 90 %, above 85 %: no new warning…"
        );
        assert_eq!(
            w.step(gib_tenths(55), &l),
            None,
            "…and none when it rises again"
        );
        assert_eq!(w.step(gib_tenths(61), &l), Some(MemoryLevel::OverSoft));
        assert_eq!(w.step(gib_tenths(50), &l), None, "below 85 %: re-armed");
        assert_eq!(w.step(gib_tenths(55), &l), Some(MemoryLevel::NearSoft));
        assert_eq!(w.step(ByteSize(8 << 30), &l), Some(MemoryLevel::OverHard));
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
