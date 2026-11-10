//! Image references (ADR 0040, 0067): what `image = …` in `context.toml`
//! names, the tags koje builds them under, their parents and the order to
//! build them in. Pure: folders and their versions are asked through
//! closures.
//!
//! | `image = …` | Meaning |
//! |---|---|
//! | `localhost/koje/<name>:<version>` | the image folder `~/.config/koje/images/<name>/`, else koje's own image of that name |
//! | `~/…` or `/…` | an image folder anywhere |
//! | anything else | a registry image, pulled by Podman |
//!
//! koje never writes `:latest`; older values (`fedora`, `koje/base:latest`)
//! are read with a hint.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::name::ImageName;

/// koje's own images and their release (ADR 0067).
pub const BUILT_IN: &[(&str, &str)] = &[("fedora", "45"), ("ubuntu", "26.04"), ("debian", "13")];

/// The image of contexts whose `context.toml` names none (written before
/// ADR 0067, which has no default).
pub const DEFAULT: &str = "fedora";

/// The version of a new image folder.
pub const FIRST_VERSION: &str = "1";

/// What older kojes wrote for their single base image (ADR 0023).
const LEGACY: &[&str] = &[
    "koje/base:latest",
    "koje/base",
    "localhost/koje/base:latest",
];

/// Where koje's own images live in Podman's store.
pub const PREFIX: &str = "localhost/koje/";

/// koje's release of built-in image `name`.
pub fn built_in_version(name: &str) -> Option<&'static str> {
    BUILT_IN.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

pub fn is_built_in(name: &str) -> bool {
    built_in_version(name).is_some()
}

/// A version (tag) as Podman takes it: 1–128 of `A-Za-z0-9_.-`, not
/// starting with `.` or `-`; never `latest` (ADR 0067).
pub fn check_version(version: &str) -> Result<()> {
    let bad = |reason: &str| Error::Image {
        value: version.to_owned(),
        reason: reason.to_owned(),
    };
    if version.is_empty() || version.len() > 128 {
        return Err(bad("a version is 1 to 128 characters"));
    }
    if version.starts_with(['.', '-'])
        || !version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    {
        return Err(bad("a version is letters, digits, _, . and -"));
    }
    if version == "latest" {
        return Err(bad("koje names versions, not latest"));
    }
    Ok(())
}

/// `localhost/koje/<name>:<version>`.
pub fn reference(name: &str, version: &str) -> String {
    format!("{PREFIX}{name}:{version}")
}

/// Label: the hash of an image folder's files at build time.
pub const LABEL_SOURCE: &str = "io.koje.source";
/// Label: the IDs of the images an image was built on (its parents, or for
/// the user layer its image), `,`-separated.
pub const LABEL_PARENTS: &str = "io.koje.parents";
/// Label: the version of koje's user layer an image carries.
pub const LABEL_USER_LAYER: &str = "io.koje.user-layer";
/// Label: the image's GBM backend directory (ADR 0016), found when the user
/// layer is built.
pub const LABEL_GBM_DIR: &str = "io.koje.gbm-dir";

/// Label: the tools the probe did not find in the image, `,`-separated.
pub const LABEL_MISSING: &str = "io.koje.missing";

/// What koje runs in an image before it adds the user layer (as root, `sh`):
/// one `missing <tool>` line per tool it lacks, and `gbm <dir>`, where its
/// GBM looks for backends.
pub const PROBE: &str = r#"PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
for t in useradd dbus-daemon podman fuse-overlayfs newuidmap newgidmap setcap sudo; do
  command -v "$t" >/dev/null 2>&1 || echo "missing $t"
done
for d in /usr/lib64/gbm /usr/lib/x86_64-linux-gnu/gbm /usr/lib/aarch64-linux-gnu/gbm /usr/lib/gbm; do
  if [ -d "$d" ]; then echo "gbm $d"; exit 0; fi
done
if [ -d /usr/lib64 ]; then echo "gbm /usr/lib64/gbm"
elif [ -d /usr/lib/x86_64-linux-gnu ]; then echo "gbm /usr/lib/x86_64-linux-gnu/gbm"
else echo "gbm /usr/lib/gbm"; fi
"#;

/// [`PROBE`]'s findings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probe {
    pub missing: BTreeSet<String>,
    pub gbm_dir: Option<String>,
}

/// Read [`PROBE`]'s output.
pub fn read_probe(out: &str) -> Probe {
    let mut probe = Probe::default();
    for line in out.lines() {
        if let Some(tool) = line.strip_prefix("missing ") {
            probe.missing.insert(tool.trim().to_owned());
        } else if let Some(dir) = line.strip_prefix("gbm ") {
            probe.gbm_dir = Some(dir.trim().to_owned());
        }
    }
    probe
}

/// Tools without which no context runs on an image: the user layer needs
/// `useradd`, every context runs `dbus-daemon` as its main process.
const REQUIRED: &[(&str, &str)] = &[
    ("useradd", "koje adds your user with it"),
    ("dbus-daemon", "it runs every context's session bus"),
];

/// Tools an access switch needs (ADR 0014: nested Podman).
const BY_SWITCH: &[(&str, &[&str])] = &[
    (
        "nested",
        &[
            "podman",
            "fuse-overlayfs",
            "newuidmap",
            "newgidmap",
            "setcap",
        ],
    ),
    ("sudo", &["sudo"]),
];

/// Why an image cannot be a context's image; empty when it can.
pub fn blockers(missing: &BTreeSet<String>) -> Vec<String> {
    REQUIRED
        .iter()
        .filter(|(tool, _)| missing.contains(*tool))
        .map(|(tool, why)| format!("the image has no {tool}: {why}"))
        .collect()
}

/// What a context with `access` lacks in an image that misses `missing`.
pub fn warnings(missing: &BTreeSet<String>, access: &crate::access::Access) -> Vec<String> {
    let on: BTreeSet<&str> = access
        .switches()
        .into_iter()
        .filter(|(_, on)| *on)
        .map(|(n, _)| n)
        .collect();
    BY_SWITCH
        .iter()
        .filter(|(switch, _)| on.contains(switch))
        .filter_map(|(switch, tools)| {
            let lacking: Vec<&str> = tools
                .iter()
                .copied()
                .filter(|t| missing.contains(*t))
                .collect();
            (!lacking.is_empty()).then(|| {
                format!(
                    "{switch}: the image has no {} – install them in the image or turn {switch} off",
                    lacking.join(", ")
                )
            })
        })
        .collect()
}

/// The image folder a context gets as its own (ADR 0063): named after it,
/// unless that is a built-in image's name, which it would take the place
/// of for every context.
pub fn own_image(ctx: &crate::name::ContextName) -> Result<ImageName> {
    let name = ctx.as_str();
    if is_built_in(name) {
        ImageName::new(format!("{name}-own"))
    } else {
        ImageName::new(name)
    }
}

/// `image = …` as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageRef {
    /// One of koje's: an image folder in `images/`, else a built-in image;
    /// `version` when the reference names one.
    Koje {
        name: ImageName,
        version: Option<String>,
    },
    /// An image folder at a path, `~/` not yet expanded.
    Path(String),
    /// A registry image.
    Registry(String),
}

impl ImageRef {
    /// koje's image `name`, any version.
    pub fn koje(name: &ImageName) -> Self {
        Self::Koje {
            name: name.clone(),
            version: None,
        }
    }

    /// The image name of a koje reference.
    pub fn koje_name(&self) -> Option<&ImageName> {
        match self {
            Self::Koje { name, .. } => Some(name),
            _ => None,
        }
    }
}

/// [`parse`]'s result: the reference and a hint for the user, e.g. that an
/// old value was read as a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub image: ImageRef,
    pub hint: Option<String>,
}

/// Read `image = …` (or what a `FROM` line names).
pub fn parse(value: &str) -> Result<Parsed> {
    let v = value.trim();
    if v.is_empty() {
        return Err(Error::Image {
            value: value.to_owned(),
            reason: "must not be empty".into(),
        });
    }
    let short = |name: ImageName, was: &str| {
        let hint = format!(
            "image = {was:?} is read as {}; write that (koje Settings and `koje edit` do)",
            match built_in_version(name.as_str()) {
                Some(v) => reference(name.as_str(), v),
                None => format!("{PREFIX}{name}:<its version>"),
            }
        );
        Parsed {
            image: ImageRef::koje(&name),
            hint: Some(hint),
        }
    };
    if LEGACY.contains(&v) {
        return Ok(short(ImageName::default_image(), v));
    }
    if v.starts_with("~/") || v.starts_with('/') {
        return Ok(Parsed {
            image: ImageRef::Path(v.to_owned()),
            hint: None,
        });
    }
    if let Some(rest) = v.strip_prefix(PREFIX)
        && !rest.starts_with("user/")
    {
        let (name, version) = match rest.split_once(':') {
            Some((n, t)) => (n, Some(t)),
            None => (rest, None),
        };
        let name = ImageName::new(name).map_err(|e| Error::Image {
            value: v.to_owned(),
            reason: e.to_string(),
        })?;
        if let Some(t) = version {
            check_version(t).map_err(|e| Error::Image {
                value: v.to_owned(),
                reason: e.to_string(),
            })?;
        }
        return Ok(Parsed {
            image: ImageRef::Koje {
                name,
                version: version.map(str::to_owned),
            },
            hint: None,
        });
    }
    if let Ok(name) = ImageName::new(v) {
        return Ok(short(name, v));
    }
    Ok(Parsed {
        image: ImageRef::Registry(v.to_owned()),
        hint: None,
    })
}

/// Where an image comes from, resolved.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// An image folder: its `Containerfile` (or `Dockerfile`) is built with
    /// the folder as build context. `name` is its tag's name.
    Folder {
        name: String,
        version: String,
        dir: PathBuf,
    },
    /// An image built into koje, in koje's release.
    BuiltIn { name: ImageName, version: String },
    /// Pulled by Podman.
    Registry(String),
}

impl Source {
    /// The image's name in Podman's store; for koje's images also what
    /// `image = …` and `FROM` name.
    pub fn tag(&self) -> String {
        match self {
            Self::Folder { name, version, .. } => reference(name, version),
            Self::BuiltIn { name, version } => reference(name.as_str(), version),
            Self::Registry(reference) => reference.clone(),
        }
    }

    /// The image with koje's user layer on top: what a context runs.
    pub fn user_tag(&self) -> String {
        let (key, version) = match self {
            Self::Folder { name, version, .. } => (name.clone(), version.as_str()),
            Self::BuiltIn { name, version } => (name.to_string(), version.as_str()),
            Self::Registry(reference) => (keyed("registry", reference, reference), FIRST_VERSION),
        };
        format!("{PREFIX}user/{key}:{version}")
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Folders at a path have a made-up name: their path says more.
            Self::Folder { name, dir, .. } if name.starts_with("path-") => {
                write!(f, "image folder {}", dir.display())
            }
            Self::Folder { .. } | Self::BuiltIn { .. } => write!(f, "image {}", self.tag()),
            Self::Registry(reference) => write!(f, "registry image {reference}"),
        }
    }
}

/// Where `image` comes from: `images_dir` is `~/.config/koje/images`, `home`
/// expands `~/`, `is_dir` says whether a folder exists, `version_of` an
/// image folder's version. A version the image does not have is an error.
pub fn resolve(
    image: &ImageRef,
    images_dir: &Path,
    home: &Path,
    is_dir: impl Fn(&Path) -> bool,
    version_of: impl Fn(&Path) -> String,
) -> Result<Source> {
    match image {
        ImageRef::Koje { name, version } => {
            let dir = images_dir.join(name.as_str());
            let (src, have) = if is_dir(&dir) {
                // A customized koje image keeps koje's release (ADR 0065).
                let v =
                    built_in_version(name.as_str()).map_or_else(|| version_of(&dir), str::to_owned);
                (
                    Source::Folder {
                        name: name.to_string(),
                        version: v.clone(),
                        dir,
                    },
                    v,
                )
            } else if let Some(v) = built_in_version(name.as_str()) {
                (
                    Source::BuiltIn {
                        name: name.clone(),
                        version: v.to_owned(),
                    },
                    v.to_owned(),
                )
            } else {
                return Err(Error::Image {
                    value: name.to_string(),
                    reason: format!(
                        "no image folder {} and no image of that name built into koje ({})",
                        dir.display(),
                        BUILT_IN
                            .iter()
                            .map(|(n, v)| reference(n, v))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                });
            };
            if let Some(want) = version
                && *want != have
            {
                return Err(Error::Image {
                    value: reference(name.as_str(), want),
                    reason: format!("there is {} – write that", src.tag()),
                });
            }
            Ok(src)
        }
        ImageRef::Path(written) => {
            let dir = match written.strip_prefix("~/") {
                Some(rest) => home.join(rest),
                None => PathBuf::from(written),
            };
            if !is_dir(&dir) {
                return Err(Error::Image {
                    value: written.clone(),
                    reason: format!("{} is not a folder", dir.display()),
                });
            }
            let base = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let name = keyed("path", &base, &dir.to_string_lossy());
            let version = version_of(&dir);
            Ok(Source::Folder { name, version, dir })
        }
        ImageRef::Registry(reference) => Ok(Source::Registry(reference.clone())),
    }
}

/// `<kind>-<readable part of label>-<hash of key>`: a tag name for images
/// that have no image name of their own.
fn keyed(kind: &str, label: &str, key: &str) -> String {
    let readable: String = label
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(24)
        .collect();
    let readable = readable.trim_matches('-');
    let hash = &hash_bytes(key.as_bytes())[..8];
    if readable.is_empty() {
        format!("{kind}-{hash}")
    } else {
        format!("{kind}-{readable}-{hash}")
    }
}

/// The koje images a Containerfile builds on (`FROM localhost/koje/…`), in
/// order, each once. Not the user layer's images.
pub fn parents(containerfile: &str) -> Vec<ImageRef> {
    let mut out: Vec<ImageRef> = Vec::new();
    for image in from_images(containerfile) {
        if let Ok(p) = parse(image)
            && p.hint.is_none()
            && matches!(p.image, ImageRef::Koje { .. })
            && !out.contains(&p.image)
        {
            out.push(p.image);
        }
    }
    out
}

/// What each `FROM` line names.
fn from_images(containerfile: &str) -> impl Iterator<Item = &str> {
    containerfile.lines().filter_map(|line| {
        let mut words = line.split_whitespace();
        if !words.next().is_some_and(|w| w.eq_ignore_ascii_case("FROM")) {
            return None;
        }
        words.find(|w| !w.starts_with("--"))
    })
}

/// `containerfile` with `FROM` lines naming koje image `old` (any version)
/// naming `new` instead (a rename or a new version, ADR 0067).
pub fn with_from(containerfile: &str, old: &ImageName, new: &str) -> String {
    let mut out = String::with_capacity(containerfile.len());
    for line in containerfile.split_inclusive('\n') {
        let names_old = from_images(line).next().is_some_and(|image| {
            parse(image).is_ok_and(|p| p.hint.is_none() && p.image.koje_name() == Some(old))
        });
        if names_old && let Some(image) = from_images(line).next() {
            out.push_str(&line.replacen(image, new, 1));
        } else {
            out.push_str(line);
        }
    }
    out
}

/// The order to build `root` and the images it builds on in: parents first,
/// each once. `parents_of` gives an image's direct parents. Fails on a
/// cycle (`FROM` itself, directly or not).
pub fn build_order<T: Clone + Ord + fmt::Display>(
    root: &T,
    mut parents_of: impl FnMut(&T) -> Result<Vec<T>>,
) -> Result<Vec<T>> {
    fn visit<T: Clone + Ord + fmt::Display>(
        node: &T,
        parents_of: &mut dyn FnMut(&T) -> Result<Vec<T>>,
        path: &mut Vec<T>,
        done: &mut BTreeSet<T>,
        order: &mut Vec<T>,
    ) -> Result<()> {
        if done.contains(node) {
            return Ok(());
        }
        if path.contains(node) {
            let chain: Vec<String> = path
                .iter()
                .chain(std::iter::once(node))
                .map(ToString::to_string)
                .collect();
            return Err(Error::Image {
                value: node.to_string(),
                reason: format!("builds on itself: {}", chain.join(" → ")),
            });
        }
        path.push(node.clone());
        for parent in parents_of(node)? {
            visit(&parent, parents_of, path, done, order)?;
        }
        path.pop();
        done.insert(node.clone());
        order.push(node.clone());
        Ok(())
    }
    let mut order = Vec::new();
    visit(
        root,
        &mut parents_of,
        &mut Vec::new(),
        &mut BTreeSet::new(),
        &mut order,
    )?;
    Ok(order)
}

/// The hash of an image folder's files (relative path and content), for
/// [`LABEL_SOURCE`]: a changed file means a new build.
pub fn source_hash(files: &[(String, Vec<u8>)]) -> String {
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut bytes = Vec::new();
    for (path, content) in sorted {
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&(content.len() as u64).to_le_bytes());
        bytes.extend_from_slice(content);
    }
    hash_bytes(&bytes)
}

/// FNV-1a, 64 bit, as hex: stable across koje versions and platforms.
fn hash_bytes(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// The file Podman builds in an image folder: `Containerfile`, else
/// `Dockerfile`.
pub fn containerfile(dir: &Path, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    ["Containerfile", "Dockerfile"]
        .iter()
        .map(|f| dir.join(f))
        .find(|p| exists(p))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn name(s: &str) -> ImageName {
        ImageName::new(s).unwrap()
    }

    fn koje(n: &str, v: Option<&str>) -> ImageRef {
        ImageRef::Koje {
            name: name(n),
            version: v.map(str::to_owned),
        }
    }

    fn v1(_: &Path) -> String {
        FIRST_VERSION.to_owned()
    }

    #[test]
    fn parses_koje_images_paths_and_registry_images() {
        assert_eq!(
            parse("localhost/koje/fedora:45").unwrap(),
            Parsed {
                image: koje("fedora", Some("45")),
                hint: None
            }
        );
        assert_eq!(
            parse("localhost/koje/dev").unwrap().image,
            koje("dev", None)
        );
        assert_eq!(
            parse("~/src/app/image").unwrap().image,
            ImageRef::Path("~/src/app/image".into())
        );
        assert_eq!(
            parse("/opt/img").unwrap().image,
            ImageRef::Path("/opt/img".into())
        );
        for registry in [
            "ghcr.io/koje/fedora:44",
            "docker.io/library/debian:13",
            "debian:13",
            "Fedora",
            "localhost/koje/user/fedora:45",
        ] {
            assert_eq!(
                parse(registry).unwrap().image,
                ImageRef::Registry(registry.into()),
                "{registry}"
            );
        }
        assert!(parse("  ").is_err());
        assert!(
            parse("localhost/koje/fedora:latest").is_err(),
            "never latest"
        );
        assert!(parse("localhost/koje/Fedora:45").is_err());
    }

    #[test]
    fn reads_short_and_old_names_with_a_hint() {
        for old in [
            "koje/base:latest",
            "koje/base",
            "localhost/koje/base:latest",
            "fedora",
        ] {
            let p = parse(old).unwrap();
            assert_eq!(p.image, koje("fedora", None), "{old}");
            assert!(p.hint.unwrap().contains("localhost/koje/fedora:45"));
        }
        let dev = parse("dev").unwrap();
        assert_eq!(dev.image, koje("dev", None));
        assert!(
            dev.hint
                .unwrap()
                .contains("localhost/koje/dev:<its version>")
        );
    }

    #[test]
    fn versions_are_named_never_latest() {
        for ok in ["45", "26.04", "1", "2025-11_b"] {
            check_version(ok).unwrap();
        }
        for bad in ["", "latest", ".1", "-x", "a b", "1:2"] {
            assert!(check_version(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn koje_images_resolve_with_their_version() {
        let images = Path::new("/c/koje/images");
        let home = Path::new("/home/mj");
        let none = |_: &Path| false;
        let fedora = resolve(&koje("fedora", Some("45")), images, home, none, v1).unwrap();
        assert_eq!(fedora.tag(), "localhost/koje/fedora:45");
        assert_eq!(fedora.user_tag(), "localhost/koje/user/fedora:45");
        assert_eq!(fedora.to_string(), "image localhost/koje/fedora:45");
        let ubuntu = resolve(&koje("ubuntu", None), images, home, none, v1).unwrap();
        assert_eq!(ubuntu.tag(), "localhost/koje/ubuntu:26.04");
        let err = resolve(&koje("fedora", Some("44")), images, home, none, v1).unwrap_err();
        assert!(
            err.to_string()
                .contains("there is localhost/koje/fedora:45"),
            "{err}"
        );
        // A folder takes a built-in image's place, in koje's release.
        let own = resolve(
            &koje("fedora", Some("45")),
            images,
            home,
            |p| p == images.join("fedora"),
            v1,
        )
        .unwrap();
        assert_eq!(
            own,
            Source::Folder {
                name: "fedora".into(),
                version: "45".into(),
                dir: images.join("fedora")
            }
        );
        // Own folders: their version.
        let web = |p: &Path| p == images.join("web");
        let three = |_: &Path| "3".to_owned();
        let src = resolve(&koje("web", Some("3")), images, home, web, three).unwrap();
        assert_eq!(src.tag(), "localhost/koje/web:3");
        assert!(resolve(&koje("web", Some("2")), images, home, web, three).is_err());
        let err = resolve(&koje("dev", None), images, home, none, v1).unwrap_err();
        assert!(err.to_string().contains("/c/koje/images/dev"), "{err}");
    }

    #[test]
    fn path_folders_and_registry_images_get_tags_of_their_own() {
        let images = Path::new("/c/koje/images");
        let home = Path::new("/home/mj");
        let src = resolve(
            &ImageRef::Path("~/src/App Image".into()),
            images,
            home,
            |p| p == Path::new("/home/mj/src/App Image"),
            v1,
        )
        .unwrap();
        let Source::Folder { name: tag, dir, .. } = &src else {
            panic!("{src:?}")
        };
        assert_eq!(dir, Path::new("/home/mj/src/App Image"));
        assert!(
            tag.starts_with("path-app-image-") && tag.len() == 23,
            "{tag}"
        );
        assert_eq!(src.tag(), format!("localhost/koje/{tag}:1"));
        assert_eq!(src.user_tag(), format!("localhost/koje/user/{tag}:1"));
        assert!(resolve(&ImageRef::Path("/nope".into()), images, home, |_| false, v1).is_err());

        let reg = Source::Registry("docker.io/library/debian:13".into());
        assert_eq!(reg.tag(), "docker.io/library/debian:13");
        let user = reg.user_tag();
        assert!(
            user.starts_with("localhost/koje/user/registry-debian-13-"),
            "{user}"
        );
        assert!(user.ends_with(":1"), "{user}");
        assert_ne!(user, Source::Registry("debian:13".into()).user_tag());
    }

    #[test]
    fn finds_koje_parents_in_from_lines() {
        let file = "# FROM localhost/koje/commented:1\n\
                    FROM --platform=linux/amd64 localhost/koje/fedora:45 AS base\n\
                    RUN echo FROM localhost/koje/not-a-from:1\n\
                    from localhost/koje/dev:2\n\
                    FROM docker.io/library/debian:13\n\
                    FROM localhost/koje/fedora:45\n\
                    FROM localhost/koje/user/fedora:45\n\
                    COPY --from=base /x /x\n";
        assert_eq!(
            parents(file),
            [koje("fedora", Some("45")), koje("dev", Some("2"))]
        );
    }

    #[test]
    fn from_lines_follow_a_rename() {
        let file = "# my image\nFROM localhost/koje/web:1 AS base\nRUN echo localhost/koje/web:1\n\
                    FROM localhost/koje/webby:1\nFROM localhost/koje/web:1\n";
        let out = with_from(file, &name("web"), "localhost/koje/site:2");
        assert_eq!(
            out,
            "# my image\nFROM localhost/koje/site:2 AS base\nRUN echo localhost/koje/web:1\n\
             FROM localhost/koje/webby:1\nFROM localhost/koje/site:2\n"
        );
        assert_eq!(with_from("FROM x\n", &name("web"), "y"), "FROM x\n");
    }

    #[test]
    fn builds_parents_first_and_refuses_cycles() {
        let graph: BTreeMap<&str, Vec<&str>> = BTreeMap::from([
            ("app", vec!["dev", "fedora"]),
            ("dev", vec!["fedora"]),
            ("fedora", vec![]),
        ]);
        let order = build_order(&"app", |n| Ok(graph[n].clone())).unwrap();
        assert_eq!(order, ["fedora", "dev", "app"]);

        let cyclic: BTreeMap<&str, Vec<&str>> =
            BTreeMap::from([("a", vec!["b"]), ("b", vec!["a"]), ("self", vec!["self"])]);
        let err = build_order(&"a", |n| Ok(cyclic[n].clone())).unwrap_err();
        assert!(err.to_string().contains("a → b → a"), "{err}");
        assert!(build_order(&"self", |n| Ok(cyclic[n].clone())).is_err());
    }

    #[test]
    fn source_hash_follows_files_not_their_order() {
        let a = ("Containerfile".to_owned(), b"FROM x\n".to_vec());
        let b = ("conf/x.toml".to_owned(), b"y".to_vec());
        let h = source_hash(&[a.clone(), b.clone()]);
        assert_eq!(h.len(), 16);
        assert_eq!(h, source_hash(&[b.clone(), a.clone()]));
        assert_ne!(h, source_hash(std::slice::from_ref(&a)));
        let changed = ("conf/x.toml".to_owned(), b"z".to_vec());
        assert_ne!(h, source_hash(&[a, changed]));
    }

    #[test]
    fn reads_the_probe_and_warns_per_switch() {
        let p = read_probe("missing podman\nmissing setcap\ngbm /usr/lib/x86_64-linux-gnu/gbm\n");
        assert_eq!(p.gbm_dir.as_deref(), Some("/usr/lib/x86_64-linux-gnu/gbm"));
        assert_eq!(blockers(&p.missing), Vec::<String>::new());
        let full = crate::access::Profile::Full.access();
        assert_eq!(
            warnings(&p.missing, &full),
            [
                "nested: the image has no podman, setcap – install them in the image or turn nested off"
            ]
        );
        let no_sudo = read_probe("missing sudo\n");
        assert_eq!(
            warnings(&no_sudo.missing, &full),
            ["sudo: the image has no sudo – install them in the image or turn sudo off"]
        );
        let isolated = crate::access::Profile::Isolated.access();
        assert_eq!(warnings(&p.missing, &isolated), Vec::<String>::new());
        let bare = read_probe("missing useradd\nmissing dbus-daemon\n");
        assert_eq!(blockers(&bare.missing).len(), 2);
        assert_eq!(bare.gbm_dir, None);
    }

    #[test]
    fn a_contexts_own_image_is_named_after_it() {
        assert!(is_built_in("debian") && !is_built_in("web"));
        let ctx = |s: &str| crate::name::ContextName::new(s).unwrap();
        assert_eq!(
            own_image(&ctx("kundenportal")).unwrap(),
            name("kundenportal")
        );
        assert_eq!(own_image(&ctx("fedora")).unwrap(), name("fedora-own"));
    }

    #[test]
    fn prefers_containerfile_over_dockerfile() {
        let dir = Path::new("/i/dev");
        assert_eq!(
            containerfile(dir, |_| true),
            Some(dir.join("Containerfile"))
        );
        assert_eq!(
            containerfile(dir, |p| p.ends_with("Dockerfile")),
            Some(dir.join("Dockerfile"))
        );
        assert_eq!(containerfile(dir, |_| false), None);
    }
}
