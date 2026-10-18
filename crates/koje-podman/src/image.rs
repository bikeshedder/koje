//! koje's base image `koje/base` (ADR 0023): built locally per user from the
//! Containerfile in `assets/image/`, which is compiled into koje so an installed
//! binary can build without the repository.

use std::path::{Path, PathBuf};

use crate::{Error, Podman, Result, Runner};

/// The image contexts use by default (`context.toml` `image`).
pub const BASE_IMAGE: &str = "koje/base:latest";

const CONTAINERFILE: &str = include_str!("../../../assets/image/Containerfile");
const CONTAINERS_CONF: &str = include_str!("../../../assets/image/containers.conf");

/// The user the image is built for: contexts run with `--userns=keep-id` and
/// mount the home at the host path, so the image has the same user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageUser {
    pub name: String,
    pub uid: u32,
    pub home: PathBuf,
}

/// `podman build` arguments for the base image in `context_dir`.
pub fn build_args(user: &ImageUser, context_dir: &Path, no_cache: bool) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "build".into(),
        "--tag".into(),
        crate::image_ref(BASE_IMAGE),
        "--build-arg".into(),
        format!("USER_NAME={}", user.name),
        "--build-arg".into(),
        format!("USER_UID={}", user.uid),
        "--build-arg".into(),
        format!("USER_HOME={}", user.home.display()),
    ];
    if no_cache {
        a.push("--no-cache".into());
    }
    a.push(context_dir.display().to_string());
    a
}

impl<R: Runner> Podman<R> {
    /// Whether an image exists locally.
    pub fn image_exists(&self, image: &str) -> Result<bool> {
        let image = crate::image_ref(image);
        let out = self
            .runner
            .run(&["image", "exists", &image])
            .map_err(Error::Spawn)?;
        Ok(out.success)
    }

    /// Build `koje/base` for `user`, with podman's output on the terminal.
    pub fn build_base(&self, user: &ImageUser, no_cache: bool) -> Result<()> {
        let dir = std::env::temp_dir().join(format!("koje-image-{}", std::process::id()));
        let write = |name: &str, text: &str| {
            std::fs::write(dir.join(name), text).map_err(|source| Error::Read {
                path: dir.join(name),
                source,
            })
        };
        std::fs::create_dir_all(&dir).map_err(|source| Error::Read {
            path: dir.clone(),
            source,
        })?;
        let result = write("Containerfile", CONTAINERFILE)
            .and_then(|()| write("containers.conf", CONTAINERS_CONF))
            .and_then(|()| {
                let args = build_args(user, &dir, no_cache);
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                match self.runner.run_attached(&args) {
                    Ok(true) => Ok(()),
                    Ok(false) => Err(Error::Failed {
                        args: args.join(" "),
                        stderr: "see the build output above".into(),
                    }),
                    Err(e) => Err(Error::Spawn(e)),
                }
            });
        let _ = std::fs::remove_dir_all(&dir);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> ImageUser {
        ImageUser {
            name: "mj".into(),
            uid: 1000,
            home: "/home/mj".into(),
        }
    }

    #[test]
    fn build_args_pass_the_user() {
        let args = build_args(&user(), Path::new("/tmp/ctx"), false).join(" ");
        assert_eq!(
            args,
            "build --tag localhost/koje/base:latest --build-arg USER_NAME=mj \
             --build-arg USER_UID=1000 --build-arg USER_HOME=/home/mj /tmp/ctx"
        );
        assert!(build_args(&user(), Path::new("/x"), true).contains(&"--no-cache".to_owned()));
    }

    #[test]
    fn embedded_files_are_the_image_sources() {
        assert!(CONTAINERFILE.contains("FROM registry.fedoraproject.org/fedora:44"));
        assert!(CONTAINERFILE.contains("rpm --restore shadow-utils"));
        assert!(CONTAINERFILE.contains("COPY containers.conf"));
        assert!(CONTAINERS_CONF.contains("volumes = [\"/proc:/proc\"]"));
    }
}
