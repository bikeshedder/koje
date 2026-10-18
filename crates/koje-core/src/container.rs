//! Names koje uses for a context's container resources, so the CLI, kojed and
//! tests agree on them.

use crate::name::ContextName;

/// Label carrying the context name on every container koje creates.
pub const LABEL_CONTEXT: &str = "io.koje.context";

/// Label on containers created by tests; only those may be touched by tests.
pub const LABEL_TEST: &str = "io.koje.test";

/// The context container: `koje-<context>`.
pub fn container_name(ctx: &ContextName) -> String {
    format!("koje-{ctx}")
}

/// The context's home volume, mounted at the host's home path: `koje-<context>-home`.
pub fn home_volume(ctx: &ContextName) -> String {
    format!("koje-{ctx}-home")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let kp = ContextName::new("kundenportal").unwrap();
        assert_eq!(container_name(&kp), "koje-kundenportal");
        assert_eq!(home_volume(&kp), "koje-kundenportal-home");
    }
}
