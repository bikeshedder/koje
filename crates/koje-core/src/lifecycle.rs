//! Decisions for starting and stopping a context container (`koje up` /
//! `koje down`). Pure: the CLI asks the runtime for the status and executes
//! the returned steps.
//!
//! The container persists between `down` and `up` (its writable layer keeps
//! what the user installed); only `koje rm` removes it.

use crate::ports::ContainerStatus;

/// What `koje up` does with the context container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpPlan {
    /// No container yet: create it from the spec, then start it.
    CreateAndStart,
    /// Stopped: start it.
    Start,
    /// Already running: nothing to start (state switching is M5.9).
    AlreadyRunning,
    /// Paused: `koje wake` resumes it; `up` does not.
    Paused,
}

pub fn plan_up(status: ContainerStatus) -> UpPlan {
    match status {
        ContainerStatus::Missing => UpPlan::CreateAndStart,
        ContainerStatus::Stopped => UpPlan::Start,
        ContainerStatus::Running => UpPlan::AlreadyRunning,
        ContainerStatus::Paused => UpPlan::Paused,
    }
}

/// What `koje down` does with the context container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownPlan {
    /// Running: stop it.
    Stop,
    /// Paused: resume, then stop – so programs can shut down in order.
    UnpauseAndStop,
    /// Nothing runs.
    AlreadyStopped,
    /// The context never ran.
    NotCreated,
}

pub fn plan_down(status: ContainerStatus) -> DownPlan {
    match status {
        ContainerStatus::Running => DownPlan::Stop,
        ContainerStatus::Paused => DownPlan::UnpauseAndStop,
        ContainerStatus::Stopped => DownPlan::AlreadyStopped,
        ContainerStatus::Missing => DownPlan::NotCreated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_follows_status() {
        assert_eq!(plan_up(ContainerStatus::Missing), UpPlan::CreateAndStart);
        assert_eq!(plan_up(ContainerStatus::Stopped), UpPlan::Start);
        assert_eq!(plan_up(ContainerStatus::Running), UpPlan::AlreadyRunning);
        assert_eq!(plan_up(ContainerStatus::Paused), UpPlan::Paused);
    }

    #[test]
    fn down_follows_status() {
        assert_eq!(plan_down(ContainerStatus::Running), DownPlan::Stop);
        assert_eq!(plan_down(ContainerStatus::Paused), DownPlan::UnpauseAndStop);
        assert_eq!(
            plan_down(ContainerStatus::Stopped),
            DownPlan::AlreadyStopped
        );
        assert_eq!(plan_down(ContainerStatus::Missing), DownPlan::NotCreated);
    }
}
