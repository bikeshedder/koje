//! Names koje uses for a context's container resources, so the CLI, kojed and
//! tests agree on them.

use serde::Serialize;

use crate::access::Access;
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

/// Label with the fingerprint of the settings a container was created with
/// (M5.11 part 2, ADR 0033): image and access. Limits change live (ADR 0035).
pub const LABEL_CONFIG: &str = "io.koje.config";

/// The fingerprint of the settings that only a new container can apply. Only
/// what the user configures – host details (sockets present, driver files)
/// would make koje ask without a reason. Mounts are live (ADR 0033, 0034).
pub fn fingerprint(image: &str, access: &Access) -> String {
    #[derive(Serialize)]
    struct Settings<'a> {
        image: &'a str,
        access: &'a Access,
    }
    let text = toml::to_string(&Settings { image, access }).unwrap_or_default();
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
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

    #[test]
    fn fingerprints() {
        use crate::access::Profile;
        let full = Profile::Full.access();
        let base = fingerprint("koje/base:latest", &full);
        assert_eq!(base.len(), 16);
        assert_eq!(base, fingerprint("koje/base:latest", &full), "stable");
        assert_ne!(base, fingerprint("fedora:44", &full));
        assert_ne!(
            base,
            fingerprint("koje/base:latest", &Profile::Isolated.access())
        );
    }
}
