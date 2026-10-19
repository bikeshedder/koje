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

/// What `koje sleep` (pause) or `koje wake` (resume) does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PausePlan {
    /// Pause / resume the container.
    Do,
    /// Already in the wanted state.
    Already,
    /// Not running at all.
    NotRunning,
}

/// `koje sleep`: freeze a running context (windows to the stash first: M5).
pub fn plan_sleep(status: ContainerStatus) -> PausePlan {
    match status {
        ContainerStatus::Running => PausePlan::Do,
        ContainerStatus::Paused => PausePlan::Already,
        ContainerStatus::Stopped | ContainerStatus::Missing => PausePlan::NotRunning,
    }
}

/// `koje wake`: resume a paused context.
pub fn plan_wake(status: ContainerStatus) -> PausePlan {
    match status {
        ContainerStatus::Paused => PausePlan::Do,
        ContainerStatus::Running => PausePlan::Already,
        ContainerStatus::Stopped | ContainerStatus::Missing => PausePlan::NotRunning,
    }
}

/// Why a command needs a running context, with the command that helps.
pub fn require_running(status: ContainerStatus, ctx: &str) -> Result<(), String> {
    match status {
        ContainerStatus::Running => Ok(()),
        ContainerStatus::Paused => {
            Err(format!("{ctx} is paused – resume it with: koje wake {ctx}"))
        }
        ContainerStatus::Stopped | ContainerStatus::Missing => Err(format!(
            "{ctx} is not running – start it with: koje up {ctx} -s empty"
        )),
    }
}

/// Which dev container `koje shell --dev [name]` enters: the requested one, or
/// the only one running.
pub fn pick_dev_container(running: &[String], requested: Option<&str>) -> Result<String, String> {
    match (requested, running) {
        (Some(name), _) if running.iter().any(|r| r == name) => Ok(name.to_owned()),
        (Some(name), []) => Err(format!("dev container {name} is not running (none is)")),
        (Some(name), _) => Err(format!(
            "dev container {name} is not running; running: {}",
            running.join(", ")
        )),
        (None, [only]) => Ok(only.clone()),
        (None, []) => Err("no dev container is running in this context".to_owned()),
        (None, _) => Err(format!(
            "several dev containers are running – pick one with --dev <name>: {}",
            running.join(", ")
        )),
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
    fn sleep_and_wake() {
        assert_eq!(plan_sleep(ContainerStatus::Running), PausePlan::Do);
        assert_eq!(plan_sleep(ContainerStatus::Paused), PausePlan::Already);
        assert_eq!(plan_sleep(ContainerStatus::Stopped), PausePlan::NotRunning);
        assert_eq!(plan_wake(ContainerStatus::Paused), PausePlan::Do);
        assert_eq!(plan_wake(ContainerStatus::Running), PausePlan::Already);
        assert_eq!(plan_wake(ContainerStatus::Missing), PausePlan::NotRunning);
    }

    #[test]
    fn running_is_required_with_a_hint() {
        assert!(require_running(ContainerStatus::Running, "kp").is_ok());
        assert!(
            require_running(ContainerStatus::Stopped, "kp")
                .unwrap_err()
                .contains("koje up kp -s empty")
        );
        assert!(
            require_running(ContainerStatus::Missing, "kp")
                .unwrap_err()
                .contains("koje up kp")
        );
        assert!(
            require_running(ContainerStatus::Paused, "kp")
                .unwrap_err()
                .contains("koje wake kp")
        );
    }

    #[test]
    fn dev_container_choice() {
        let one = vec!["web".to_owned()];
        let two = vec!["web".to_owned(), "db".to_owned()];
        assert_eq!(pick_dev_container(&one, None).unwrap(), "web");
        assert_eq!(pick_dev_container(&two, Some("db")).unwrap(), "db");
        assert!(
            pick_dev_container(&two, None)
                .unwrap_err()
                .contains("--dev <name>: web, db")
        );
        assert!(
            pick_dev_container(&[], None)
                .unwrap_err()
                .contains("no dev container")
        );
        assert!(
            pick_dev_container(&one, Some("db"))
                .unwrap_err()
                .contains("running: web")
        );
        assert!(
            pick_dev_container(&[], Some("db"))
                .unwrap_err()
                .contains("none is")
        );
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
