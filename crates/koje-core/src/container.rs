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
    // `sudo` is applied at every start (ADR 0066), not by a new container:
    // left out, so contexts made before it was a switch keep their hash.
    let mut text = String::new();
    for line in toml::to_string(&Settings { image, access })
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with("sudo ="))
    {
        text.push_str(line);
        text.push('\n');
    }
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    // Versioned: a container fingerprinted by an older koje (limits were part
    // of it) is not compared (see `fingerprint_matches`).
    format!("{FINGERPRINT_VERSION}-{hash:016x}")
}

const FINGERPRINT_VERSION: &str = "v2";

/// Whether a container's label says its settings differ from `current`;
/// `false` for labels of an older format (nothing to compare).
pub fn fingerprint_differs(label: &str, current: &str) -> bool {
    label.starts_with(&format!("{FINGERPRINT_VERSION}-")) && label != current
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
    fn fingerprints_stay_as_before_sudo_and_ignore_it() {
        use crate::access::{Access, Profile};
        // Values from before `sudo` was a switch: no recreate is asked for.
        assert_eq!(
            fingerprint("koje/base:latest", &Profile::Full.access()),
            "v2-81e848296b63c642"
        );
        assert_eq!(
            fingerprint("fedora", &Profile::Headless.access()),
            "v2-721bfc2602d0fda3"
        );
        let off = Access {
            sudo: false,
            ..Access::ALL
        };
        assert_eq!(
            fingerprint("fedora", &off),
            fingerprint("fedora", &Access::ALL)
        );
    }

    #[test]
    fn fingerprints() {
        use crate::access::Profile;
        let full = Profile::Full.access();
        let base = fingerprint("koje/base:latest", &full);
        assert!(base.starts_with("v2-") && base.len() == 19, "{base}");
        assert!(!fingerprint_differs(&base, &base));
        assert!(fingerprint_differs("v2-0000000000000000", &base));
        assert!(
            !fingerprint_differs("3f1c2a…old", &base),
            "an older koje's label"
        );
        assert_eq!(base, fingerprint("koje/base:latest", &full), "stable");
        assert_ne!(base, fingerprint("fedora:44", &full));
        assert_ne!(
            base,
            fingerprint("koje/base:latest", &Profile::Isolated.access())
        );
    }
}
