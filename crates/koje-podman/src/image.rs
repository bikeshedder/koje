//! Images (ADR 0023, 0040): koje's built-in images and its user layer, whose
//! files are compiled into koje so an installed binary builds without the
//! repository, and the Podman calls to build, pull and inspect images.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use koje_core::name::ContextName;

use crate::{Error, Podman, Result, Runner};

/// The images built into koje: name → files of its build context.
const BUILT_IN: &[(&str, &[(&str, &str)])] = &[(
    "fedora",
    &[(
        "Containerfile",
        include_str!("../../../assets/images/fedora/Containerfile"),
    )],
)];

/// koje's user layer (ADR 0040): its build context.
pub const USER_LAYER: &[(&str, &str)] = &[
    (
        "Containerfile",
        include_str!("../../../assets/images/user-layer/Containerfile"),
    ),
    (
        "user-layer.sh",
        include_str!("../../../assets/images/user-layer/user-layer.sh"),
    ),
    (
        "containers.conf",
        include_str!("../../../assets/images/user-layer/containers.conf"),
    ),
];

/// The files of built-in image `name`.
pub fn built_in(name: &str) -> Option<&'static [(&'static str, &'static str)]> {
    BUILT_IN.iter().find(|(n, _)| *n == name).map(|(_, f)| *f)
}

/// The user the user layer adds: contexts run with `--userns=keep-id` and
/// mount the home at the host path, so the image has the same user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageUser {
    pub name: String,
    pub uid: u32,
    pub home: PathBuf,
}

impl ImageUser {
    /// The user layer's build arguments.
    pub fn build_args(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("USER_NAME".to_owned(), self.name.clone()),
            ("USER_UID".to_owned(), self.uid.to_string()),
            ("USER_HOME".to_owned(), self.home.display().to_string()),
        ])
    }
}

/// One `podman build`.
#[derive(Debug, Clone)]
pub struct Build<'a> {
    pub tag: &'a str,
    /// The build context.
    pub dir: &'a Path,
    /// The Containerfile, when not `dir/Containerfile`.
    pub file: Option<&'a Path>,
    pub labels: BTreeMap<String, String>,
    pub args: BTreeMap<String, String>,
    pub no_cache: bool,
    /// Pull newer base images (`--pull=always`); else `missing`, or `never`
    /// for `local_from` (the base is one of koje's own).
    pub pull: bool,
    pub local_from: bool,
}

impl Default for Build<'_> {
    fn default() -> Self {
        Self {
            tag: "",
            dir: Path::new("."),
            file: None,
            labels: BTreeMap::new(),
            args: BTreeMap::new(),
            no_cache: false,
            pull: false,
            local_from: false,
        }
    }
}

/// `podman build` arguments for `b`.
pub fn build_args(b: &Build<'_>) -> Vec<String> {
    let mut a: Vec<String> = vec!["build".into(), "--tag".into(), b.tag.to_owned()];
    if let Some(file) = b.file {
        a.extend(["--file".into(), file.display().to_string()]);
    }
    for (k, v) in &b.args {
        a.extend(["--build-arg".into(), format!("{k}={v}")]);
    }
    for (k, v) in &b.labels {
        a.extend(["--label".into(), format!("{k}={v}")]);
    }
    let pull = if b.pull {
        "always"
    } else if b.local_from {
        "never"
    } else {
        "missing"
    };
    a.push(format!("--pull={pull}"));
    if b.no_cache {
        a.push("--no-cache".into());
    }
    a.push(b.dir.display().to_string());
    a
}

/// Write `files` into a new temporary directory (a build context).
pub fn temp_context(files: &[(&str, &str)]) -> Result<TempDir> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "koje-image-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let err = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Read { path, source }
    };
    std::fs::create_dir_all(&dir).map_err(err(&dir))?;
    let temp = TempDir(dir);
    for (name, text) in files {
        let path = temp.0.join(name);
        std::fs::write(&path, text).map_err(err(&path))?;
    }
    Ok(temp)
}

/// A directory removed when dropped.
#[derive(Debug)]
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

    /// An image's ID; `None` when there is no such image.
    pub fn image_id(&self, image: &str) -> Result<Option<String>> {
        let image = crate::image_ref(image);
        let out = self
            .runner
            .run(&["image", "inspect", "--format", "{{.Id}}", &image])
            .map_err(Error::Spawn)?;
        let id = out.stdout.trim();
        Ok((out.success && !id.is_empty()).then(|| id.to_owned()))
    }

    /// An image's labels; empty when there is no such image.
    pub fn image_labels(&self, image: &str) -> Result<BTreeMap<String, String>> {
        let image = crate::image_ref(image);
        let out = self
            .runner
            .run(&[
                "image",
                "inspect",
                "--format",
                "{{range $k, $v := .Labels}}{{$k}}={{$v}}\n{{end}}",
                &image,
            ])
            .map_err(Error::Spawn)?;
        if !out.success {
            return Ok(BTreeMap::new());
        }
        Ok(out
            .stdout
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect())
    }

    /// The ID of the image a context's container was created from.
    pub fn container_image_id(&self, ctx: &ContextName) -> Result<Option<String>> {
        let name = koje_core::container::container_name(ctx);
        let out = self
            .runner
            .run(&["container", "inspect", "--format", "{{.Image}}", &name])
            .map_err(Error::Spawn)?;
        let id = out.stdout.trim();
        Ok((out.success && !id.is_empty()).then(|| id.to_owned()))
    }

    /// Build an image; podman's output goes to `on_line`.
    pub fn build_image(&self, b: &Build<'_>, on_line: &mut dyn FnMut(&str)) -> Result<()> {
        let args = build_args(b);
        self.attached(
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
            on_line,
        )
    }

    /// Pull a registry image; podman's output goes to `on_line`.
    pub fn pull(&self, reference: &str, on_line: &mut dyn FnMut(&str)) -> Result<()> {
        self.attached(&["pull", reference], on_line)
    }

    /// Run `script` with `sh` as root in a throwaway container of `image`
    /// (no network): its output.
    pub fn probe(&self, image: &str, script: &str) -> Result<String> {
        let image = crate::image_ref(image);
        self.podman(&[
            "run",
            "--rm",
            "--user",
            "root",
            "--network",
            "none",
            "--pull",
            "never",
            "--entrypoint",
            "sh",
            &image,
            "-c",
            script,
        ])
    }

    fn attached(&self, args: &[&str], on_line: &mut dyn FnMut(&str)) -> Result<()> {
        match self.runner.run_lines(args, on_line) {
            Ok(true) => Ok(()),
            Ok(false) => Err(Error::Failed {
                args: args.join(" "),
                stderr: "see podman's output above".into(),
            }),
            Err(e) => Err(Error::Spawn(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Fake;

    #[test]
    fn build_args_carry_labels_args_and_pull_policy() {
        let b = Build {
            tag: "localhost/koje/user/fedora:latest",
            dir: Path::new("/tmp/ctx"),
            labels: BTreeMap::from([("io.koje.parents".into(), "sha256:a".into())]),
            args: BTreeMap::from([("USER_NAME".into(), "mj".into())]),
            local_from: true,
            ..Build::default()
        };
        assert_eq!(
            build_args(&b).join(" "),
            "build --tag localhost/koje/user/fedora:latest --build-arg USER_NAME=mj \
             --label io.koje.parents=sha256:a --pull=never /tmp/ctx"
        );
        let file = PathBuf::from("/i/dev/Dockerfile");
        let b = Build {
            file: Some(&file),
            no_cache: true,
            pull: true,
            ..b
        };
        let args = build_args(&b).join(" ");
        assert!(args.contains("--file /i/dev/Dockerfile"), "{args}");
        assert!(
            args.ends_with("--pull=always --no-cache /tmp/ctx"),
            "{args}"
        );
    }

    #[test]
    fn embedded_files_are_the_image_sources() {
        let fedora = built_in("fedora").unwrap();
        assert!(
            fedora[0]
                .1
                .contains("FROM registry.fedoraproject.org/fedora:44")
        );
        assert!(!fedora[0].1.contains("useradd"), "no user in the image");
        assert!(built_in("debian").is_none());
        let files: BTreeMap<_, _> = USER_LAYER.iter().copied().collect();
        assert!(files["Containerfile"].contains("FROM ${KOJE_IMAGE}"));
        assert!(files["user-layer.sh"].contains("useradd"));
        assert!(!files["user-layer.sh"].contains("rpm "), "portable");
        assert!(files["containers.conf"].contains("volumes = [\"/proc:/proc\"]"));
    }

    #[test]
    fn labels_and_ids_of_missing_images_are_empty() {
        let fake = Fake::default()
            .answer(
                true,
                "io.koje.source=abc\nio.koje.gbm-dir=/usr/lib64/gbm\n",
                "",
            )
            .answer(false, "", "Error: no such image")
            .answer(true, "sha256:1\n", "");
        let podman = Podman::with_runner(&fake, "/x", "/x");
        let labels = podman.image_labels("koje/fedora:latest").unwrap();
        assert_eq!(labels["io.koje.source"], "abc");
        assert_eq!(labels["io.koje.gbm-dir"], "/usr/lib64/gbm");
        assert!(podman.image_labels("nope").unwrap().is_empty());
        assert_eq!(
            podman.image_id("koje/x").unwrap().as_deref(),
            Some("sha256:1")
        );
        assert!(fake.calls.borrow()[0].ends_with("localhost/koje/fedora:latest"));
    }

    #[test]
    fn temp_context_holds_the_files_until_dropped() {
        let t = temp_context(&[("Containerfile", "FROM x\n")]).unwrap();
        let path = t.path().to_owned();
        assert_eq!(
            std::fs::read_to_string(path.join("Containerfile")).unwrap(),
            "FROM x\n"
        );
        drop(t);
        assert!(!path.exists());
    }
}
