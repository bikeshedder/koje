//! Ending the session as GNOME does (M8.47): log out, restart and power off
//! ask first with a 60-second countdown, then happen on their own; suspend
//! happens at once. Pure: the texts and what each one runs.

/// What ending the session means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndSession {
    LogOut,
    Restart,
    PowerOff,
}

/// GNOME Shell's countdown.
pub const COUNTDOWN: u32 = 60;

fn seconds(n: u32) -> String {
    if n == 1 {
        "1 second".to_owned()
    } else {
        format!("{n} seconds")
    }
}

impl EndSession {
    /// The shell's application action (`koje session end`, Super+Shift+E).
    pub const ALL: [Self; 3] = [Self::LogOut, Self::Restart, Self::PowerOff];

    pub fn action_name(self) -> &'static str {
        match self {
            Self::LogOut => "log-out",
            Self::Restart => "restart",
            Self::PowerOff => "power-off",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::LogOut => "Log Out",
            Self::Restart => "Restart",
            Self::PowerOff => "Power Off",
        }
    }

    /// GNOME Shell's text, counting down; `user` is the real name.
    pub fn body(self, left: u32, user: &str) -> String {
        match self {
            Self::LogOut => format!(
                "{user} will be logged out automatically in {}.",
                seconds(left)
            ),
            Self::Restart => format!(
                "The system will restart automatically in {}.",
                seconds(left)
            ),
            Self::PowerOff => format!(
                "The system will power off automatically in {}.",
                seconds(left)
            ),
        }
    }

    /// The program and arguments that do it; logging out ends Hyprland
    /// (`koje session start` cleans up after it), which the shell asks
    /// Hyprland for instead.
    pub fn command(self) -> Option<(&'static str, [&'static str; 1])> {
        match self {
            Self::LogOut => None,
            Self::Restart => Some(("systemctl", ["reboot"])),
            Self::PowerOff => Some(("systemctl", ["poweroff"])),
        }
    }
}

/// The display change's countdown (M8.53), as GNOME words it.
pub fn display_revert_text(left: u32) -> String {
    format!("Settings changes will revert in {}", seconds(left))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnomes_words() {
        assert_eq!(
            EndSession::LogOut.body(60, "Michael Jung"),
            "Michael Jung will be logged out automatically in 60 seconds."
        );
        assert_eq!(
            EndSession::PowerOff.body(1, "x"),
            "The system will power off automatically in 1 second."
        );
        assert_eq!(
            display_revert_text(20),
            "Settings changes will revert in 20 seconds"
        );
        assert_eq!(
            display_revert_text(1),
            "Settings changes will revert in 1 second"
        );
    }

    #[test]
    fn what_each_one_runs() {
        assert_eq!(EndSession::LogOut.command(), None);
        assert_eq!(
            EndSession::Restart.command(),
            Some(("systemctl", ["reboot"]))
        );
        assert_eq!(
            EndSession::PowerOff.command(),
            Some(("systemctl", ["poweroff"]))
        );
        let names: Vec<&str> = EndSession::ALL.iter().map(|e| e.action_name()).collect();
        assert_eq!(names, ["log-out", "restart", "power-off"]);
    }
}
