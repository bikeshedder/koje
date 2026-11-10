//! Image references (ADR 0040): what `image = …` in `context.toml` names,
//! the tags koje builds them under, their parents and the order to build
//! them in. Pure: whether a folder exists is asked through a closure.
//!
//! | `image = …` | Meaning |
//! |---|---|
//! | `fedora` (a plain name) | the image folder `~/.config/koje/images/fedora/`, else the built-in image |
//! | `~/…` or `/…` | an image folder anywhere |
//! | anything else | a registry image, pulled by Podman |

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::name::ImageName;

/// The image of new contexts.
pub const DEFAULT: &str = "fedora";

/// The images built into koje.
pub const BUILT_IN: &[&str] = &["fedora"];

/// What older kojes wrote for their single base image (ADR 0023).
const LEGACY: &[&str] = &[
    "koje/base:latest",
    "koje/base",
    "localhost/koje/base:latest",
];

/// Where koje's own images live in Podman's store.
const PREFIX: &str = "localhost/koje/";

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
for t in useradd dbus-daemon podman fuse-overlayfs newuidmap newgidmap setcap; do
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
const BY_SWITCH: &[(&str, &[&str])] = &[(
    "nested",
    &[
        "podman",
        "fuse-overlayfs",
        "newuidmap",
        "newgidmap",
        "setcap",
    ],
)];

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

/// `image = …` as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageRef {
    /// An image folder in `images/`, else a built-in image.
    Name(ImageName),
    /// An image folder at a path, `~/` not yet expanded.
    Path(String),
    /// A registry image.
    Registry(String),
}

/// [`parse`]'s result: the reference and a hint for the user, e.g. that an
/// old value was read as a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub image: ImageRef,
    pub hint: Option<String>,
}

/// Read `image = …`.
pub fn parse(value: &str) -> Result<Parsed> {
    let v = value.trim();
    if v.is_empty() {
        return Err(Error::Image {
            value: value.to_owned(),
            reason: "must not be empty".into(),
        });
    }
    if LEGACY.contains(&v) {
        return Ok(Parsed {
            image: ImageRef::Name(ImageName::default_image()),
            hint: Some(format!(
                "image = {v:?} is read as {DEFAULT:?}, koje's image now – write that instead"
            )),
        });
    }
    let image = if v.starts_with("~/") || v.starts_with('/') {
        ImageRef::Path(v.to_owned())
    } else if let Ok(name) = ImageName::new(v) {
        ImageRef::Name(name)
    } else {
        ImageRef::Registry(v.to_owned())
    };
    Ok(Parsed { image, hint: None })
}

/// Where an image comes from, resolved.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// An image folder: its `Containerfile` (or `Dockerfile`) is built with
    /// the folder as build context. `name` is its tag's name.
    Folder { name: String, dir: PathBuf },
    /// An image built into koje.
    BuiltIn(ImageName),
    /// Pulled by Podman.
    Registry(String),
}

impl Source {
    /// The image's name in Podman's store.
    pub fn tag(&self) -> String {
        match self {
            Self::Folder { name, .. } => format!("{PREFIX}{name}:latest"),
            Self::BuiltIn(name) => format!("{PREFIX}{name}:latest"),
            Self::Registry(reference) => reference.clone(),
        }
    }

    /// The image with koje's user layer on top: what a context runs.
    pub fn user_tag(&self) -> String {
        let key = match self {
            Self::Folder { name, .. } => name.clone(),
            Self::BuiltIn(name) => name.to_string(),
            Self::Registry(reference) => keyed("registry", reference, reference),
        };
        format!("{PREFIX}user/{key}:latest")
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Folder { dir, .. } => write!(f, "image folder {}", dir.display()),
            Self::BuiltIn(name) => write!(f, "koje's image {name}"),
            Self::Registry(reference) => write!(f, "registry image {reference}"),
        }
    }
}

/// Where `image` comes from: `images_dir` is `~/.config/koje/images`, `home`
/// expands `~/`, `is_dir` says whether a folder exists.
pub fn resolve(
    image: &ImageRef,
    images_dir: &Path,
    home: &Path,
    is_dir: impl Fn(&Path) -> bool,
) -> Result<Source> {
    match image {
        ImageRef::Name(name) => {
            let dir = images_dir.join(name.as_str());
            if is_dir(&dir) {
                Ok(Source::Folder {
                    name: name.to_string(),
                    dir,
                })
            } else if BUILT_IN.contains(&name.as_str()) {
                Ok(Source::BuiltIn(name.clone()))
            } else {
                Err(Error::Image {
                    value: name.to_string(),
                    reason: format!(
                        "no image folder {} and no image of that name built into koje ({})",
                        dir.display(),
                        BUILT_IN.join(", ")
                    ),
                })
            }
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
            Ok(Source::Folder { name, dir })
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

/// The koje images a Containerfile builds on: `FROM localhost/koje/<name>`
/// (or `koje/<name>`), in order, each once. Not the user layer's images.
pub fn parents(containerfile: &str) -> Vec<ImageName> {
    let mut out: Vec<ImageName> = Vec::new();
    for line in containerfile.lines() {
        let mut words = line.split_whitespace();
        if !words.next().is_some_and(|w| w.eq_ignore_ascii_case("FROM")) {
            continue;
        }
        let Some(image) = words.find(|w| !w.starts_with("--")) else {
            continue;
        };
        let Some(rest) = image
            .strip_prefix(PREFIX)
            .or_else(|| image.strip_prefix("koje/"))
        else {
            continue;
        };
        let rest = rest.split(['@', ':']).next().unwrap_or_default();
        if let Ok(name) = ImageName::new(rest)
            && !out.contains(&name)
        {
            out.push(name);
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

    #[test]
    fn parses_names_paths_and_registry_images() {
        assert_eq!(
            parse("fedora").unwrap().image,
            ImageRef::Name(name("fedora"))
        );
        assert_eq!(
            parse("my-dev").unwrap().image,
            ImageRef::Name(name("my-dev"))
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
        ] {
            assert_eq!(
                parse(registry).unwrap().image,
                ImageRef::Registry(registry.into()),
                "{registry}"
            );
        }
        assert!(parse("  ").is_err());
    }

    #[test]
    fn reads_the_old_base_image_as_fedora_with_a_hint() {
        for old in [
            "koje/base:latest",
            "koje/base",
            "localhost/koje/base:latest",
        ] {
            let p = parse(old).unwrap();
            assert_eq!(p.image, ImageRef::Name(name("fedora")));
            assert!(p.hint.unwrap().contains("\"fedora\""));
        }
        assert_eq!(parse("fedora").unwrap().hint, None);
    }

    #[test]
    fn a_folder_takes_a_built_in_images_place() {
        let images = Path::new("/c/koje/images");
        let home = Path::new("/home/mj");
        let none = |_: &Path| false;
        let fedora = ImageRef::Name(name("fedora"));
        assert_eq!(
            resolve(&fedora, images, home, none).unwrap(),
            Source::BuiltIn(name("fedora"))
        );
        let own = resolve(&fedora, images, home, |p| p == images.join("fedora")).unwrap();
        assert_eq!(
            own,
            Source::Folder {
                name: "fedora".into(),
                dir: images.join("fedora")
            }
        );
        let err = resolve(&ImageRef::Name(name("dev")), images, home, none).unwrap_err();
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
        )
        .unwrap();
        let Source::Folder { name: tag, dir } = &src else {
            panic!("{src:?}")
        };
        assert_eq!(dir, Path::new("/home/mj/src/App Image"));
        assert!(
            tag.starts_with("path-app-image-") && tag.len() == 23,
            "{tag}"
        );
        assert_eq!(src.tag(), format!("localhost/koje/{tag}:latest"));
        assert_eq!(src.user_tag(), format!("localhost/koje/user/{tag}:latest"));
        assert!(resolve(&ImageRef::Path("/nope".into()), images, home, |_| false).is_err());

        let reg = Source::Registry("docker.io/library/debian:13".into());
        assert_eq!(reg.tag(), "docker.io/library/debian:13");
        let user = reg.user_tag();
        assert!(
            user.starts_with("localhost/koje/user/registry-debian-13-"),
            "{user}"
        );
        assert_ne!(user, Source::Registry("debian:13".into()).user_tag());

        let fedora = Source::BuiltIn(name("fedora"));
        assert_eq!(fedora.tag(), "localhost/koje/fedora:latest");
        assert_eq!(fedora.user_tag(), "localhost/koje/user/fedora:latest");
    }

    #[test]
    fn finds_koje_parents_in_from_lines() {
        let file = "# FROM localhost/koje/commented\n\
                    FROM --platform=linux/amd64 localhost/koje/fedora:latest AS base\n\
                    RUN echo FROM localhost/koje/not-a-from\n\
                    from koje/dev\n\
                    FROM docker.io/library/debian:13\n\
                    FROM localhost/koje/fedora\n\
                    FROM localhost/koje/user/fedora:latest\n\
                    COPY --from=base /x /x\n";
        assert_eq!(parents(file), [name("fedora"), name("dev")]);
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
        let isolated = crate::access::Profile::Isolated.access();
        assert_eq!(warnings(&p.missing, &isolated), Vec::<String>::new());
        let bare = read_probe("missing useradd\nmissing dbus-daemon\n");
        assert_eq!(blockers(&bare.missing).len(), 2);
        assert_eq!(bare.gbm_dir, None);
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
