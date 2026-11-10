//! Images as koje keeps and lists them (ADR 0064): koje's built-in images
//! and the image folders in `~/.config/koje/images/<name>/`, each with its
//! `Containerfile` and an optional `image.toml` (label, description), and
//! the contexts that use them. Like [`crate::context`] for contexts.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::fsutil;
use crate::image::{self, BUILT_IN, ImageRef};
use crate::name::{ContextName, ImageName};
use crate::paths::Paths;

/// An image folder's settings file.
pub const IMAGE_TOML: &str = "image.toml";

/// `image.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageConfig {
    /// Shown instead of the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// What koje says about its built-in images.
const BUILT_IN_TEXT: &[(&str, &str, &str)] = &[(
    "fedora",
    "Fedora",
    "koje's image: Fedora with Firefox, foot, Nautilus, Podman and fonts",
)];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    BuiltIn,
    Folder,
}

/// One listed image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: ImageName,
    pub kind: Kind,
    /// The label, else the name.
    pub label: String,
    pub description: String,
    /// The folder of an image folder.
    pub dir: Option<PathBuf>,
    /// Contexts whose `image = …` names it.
    pub used_by: Vec<ContextName>,
    /// Why its `image.toml` or Containerfile is not usable.
    pub error: Option<String>,
}

/// An image folder's `image.toml`; the default when there is none.
pub fn load_config(dir: &Path) -> Result<ImageConfig> {
    let path = dir.join(IMAGE_TOML);
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|e| Error::Parse {
            path,
            source: Box::new(e),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ImageConfig::default()),
        Err(source) => Err(Error::Read { path, source }),
    }
}

/// The image folders' names, sorted (folders whose name is no image name
/// are left out).
fn folders(paths: &Paths) -> Result<Vec<ImageName>> {
    let dir = paths.images_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(Error::Read { path: dir, source }),
    };
    let mut names: Vec<ImageName> = entries
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| ImageName::new(e.file_name().to_string_lossy().into_owned()).ok())
        .collect();
    names.sort();
    Ok(names)
}

/// Every image: koje's built-in ones (unless a folder takes their place),
/// then the folders, with the contexts using each.
pub fn list(paths: &Paths) -> Result<Vec<Entry>> {
    let contexts = crate::context::list(paths)?;
    let used_by = |name: &ImageName| -> Vec<ContextName> {
        contexts
            .iter()
            .filter(|c| {
                image::parse(&c.image).is_ok_and(|p| p.image == ImageRef::Name(name.clone()))
            })
            .map(|c| c.name.clone())
            .collect()
    };
    let folders = folders(paths)?;
    let mut out = Vec::new();
    for (name, label, description) in BUILT_IN_TEXT {
        let Ok(name) = ImageName::new(*name) else {
            continue;
        };
        if !folders.contains(&name) {
            out.push(Entry {
                used_by: used_by(&name),
                name,
                kind: Kind::BuiltIn,
                label: (*label).to_owned(),
                description: (*description).to_owned(),
                dir: None,
                error: None,
            });
        }
    }
    for name in folders {
        let dir = paths.images_dir().join(name.as_str());
        let (config, mut error) = match load_config(&dir) {
            Ok(c) => (c, None),
            Err(e) => (ImageConfig::default(), Some(e.to_string())),
        };
        if image::containerfile(&dir, Path::is_file).is_none() {
            error.get_or_insert_with(|| "it has no Containerfile or Dockerfile".to_owned());
        }
        out.push(Entry {
            used_by: used_by(&name),
            label: config.label.unwrap_or_else(|| name.to_string()),
            description: config.description.unwrap_or_default(),
            name,
            kind: Kind::Folder,
            dir: Some(dir),
            error,
        });
    }
    Ok(out)
}

/// Create the image folder `name` with `containerfile` and an `image.toml`;
/// its folder. Refuses a built-in image's name: the folder would take its
/// place for every context.
pub fn create(
    paths: &Paths,
    name: &ImageName,
    containerfile: &str,
    label: Option<&str>,
) -> Result<PathBuf> {
    if BUILT_IN.contains(&name.as_str()) {
        return Err(Error::Image {
            value: name.to_string(),
            reason: "is koje's own image – choose another name".into(),
        });
    }
    let dir = paths.images_dir().join(name.as_str());
    if dir.exists() {
        return Err(Error::Image {
            value: name.to_string(),
            reason: format!("{} exists already", dir.display()),
        });
    }
    let config = ImageConfig {
        label: label
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned),
        description: None,
    };
    fsutil::write_atomic(&dir.join("Containerfile"), containerfile.as_bytes())?;
    write_config(&dir, &config)?;
    Ok(dir)
}

/// `image.toml` with a comment on what it is.
fn write_config(dir: &Path, config: &ImageConfig) -> Result<()> {
    let body = toml::to_string(config).map_err(|e| Error::InvalidEdit(e.to_string()))?;
    let text = format!(
        "# A koje image (ADR 0064); its programs are in the Containerfile.\n\
         # label = \"…\"        shown instead of the folder's name\n\
         # description = \"…\"\n{body}"
    );
    fsutil::write_atomic(&dir.join(IMAGE_TOML), text.as_bytes())
}

/// The folder of image `name`; refuses built-in images and missing folders.
fn folder(paths: &Paths, name: &ImageName) -> Result<PathBuf> {
    let dir = paths.images_dir().join(name.as_str());
    if dir.is_dir() {
        Ok(dir)
    } else if BUILT_IN.contains(&name.as_str()) {
        Err(Error::Image {
            value: name.to_string(),
            reason: "is koje's own image and cannot be changed – make a new one from it".into(),
        })
    } else {
        Err(Error::Image {
            value: name.to_string(),
            reason: format!("there is no image folder {}", dir.display()),
        })
    }
}

/// Change an image's label or description (`Some("")` removes one; `None`
/// keeps it), comments in `image.toml` kept.
pub fn set(
    paths: &Paths,
    name: &ImageName,
    label: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    let dir = folder(paths, name)?;
    let path = dir.join(IMAGE_TOML);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            write_config(&dir, &ImageConfig::default())?;
            std::fs::read_to_string(&path).map_err(|source| Error::Read {
                path: path.clone(),
                source,
            })?
        }
        Err(source) => return Err(Error::Read { path, source }),
    };
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| Error::InvalidEdit(e.to_string()))?;
    for (key, value) in [("label", label), ("description", description)] {
        match value.map(str::trim) {
            None => {}
            Some("") => {
                doc.remove(key);
            }
            Some(v) => {
                doc[key] = toml_edit::value(v);
            }
        }
    }
    let new = doc.to_string();
    toml::from_str::<ImageConfig>(&new).map_err(|e| Error::Parse {
        path: path.clone(),
        source: Box::new(e),
    })?;
    fsutil::write_atomic(&path, new.as_bytes())
}

/// Remove an image folder; refused while a context uses it.
pub fn remove(paths: &Paths, name: &ImageName) -> Result<()> {
    let dir = folder(paths, name)?;
    let users: Vec<String> = list(paths)?
        .into_iter()
        .find(|e| &e.name == name)
        .map(|e| e.used_by.iter().map(ToString::to_string).collect())
        .unwrap_or_default();
    if !users.is_empty() {
        return Err(Error::Image {
            value: name.to_string(),
            reason: format!(
                "is used by {} – give them another image first",
                users.join(", ")
            ),
        });
    }
    std::fs::remove_dir_all(&dir).map_err(|source| Error::Write { path: dir, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(tmp: &Path) -> Paths {
        Paths::with_roots(tmp.join("c"), tmp.join("d"), tmp.join("s"))
    }

    fn n(s: &str) -> ImageName {
        ImageName::new(s).unwrap()
    }

    #[test]
    fn lists_built_in_images_and_folders_with_their_contexts() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let kp = ContextName::new("kp").unwrap();
        let shop = ContextName::new("shop").unwrap();
        crate::context::create(&p, &kp, None, None).unwrap();
        crate::context::create(&p, &shop, None, None).unwrap();
        create(
            &p,
            &n("web"),
            "FROM localhost/koje/fedora\n",
            Some("Web development"),
        )
        .unwrap();
        crate::context::set_image(&p, &shop, "web").unwrap();
        // A folder without image.toml is an image too; a broken one is shown.
        let bare = p.images_dir().join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(bare.join("Dockerfile"), "FROM x\n").unwrap();
        let broken = p.images_dir().join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join(IMAGE_TOML), "colour = 1\n").unwrap();
        std::fs::create_dir_all(p.images_dir().join("Not A Name")).unwrap();

        let all = list(&p).unwrap();
        let names: Vec<&str> = all.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["fedora", "bare", "broken", "web"]);
        let fedora = &all[0];
        assert_eq!(
            (fedora.kind, fedora.label.as_str()),
            (Kind::BuiltIn, "Fedora")
        );
        assert_eq!(fedora.used_by, [kp]);
        let web = &all[3];
        assert_eq!(web.label, "Web development");
        assert_eq!(web.used_by, [shop]);
        assert_eq!(
            web.dir.as_deref(),
            Some(p.images_dir().join("web").as_path())
        );
        assert_eq!(all[1].label, "bare");
        assert_eq!(all[1].error, None);
        assert!(
            all[2].error.as_deref().unwrap().contains("colour"),
            "{:?}",
            all[2].error
        );
    }

    #[test]
    fn a_folder_named_like_a_built_in_image_takes_its_place() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let dir = p.images_dir().join("fedora");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Containerfile"), "FROM x\n").unwrap();
        let all = list(&p).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].kind, Kind::Folder);
        // But koje does not make one.
        assert!(create(&p, &n("fedora"), "FROM x\n", None).is_err());
    }

    #[test]
    fn creates_edits_and_removes_unused_images() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let dir = create(&p, &n("web"), "FROM localhost/koje/fedora\n", Some(" Web ")).unwrap();
        assert!(create(&p, &n("web"), "FROM x\n", None).is_err());
        assert_eq!(load_config(&dir).unwrap().label.as_deref(), Some("Web"));

        set(&p, &n("web"), None, Some("Node and VS Code")).unwrap();
        let text = std::fs::read_to_string(dir.join(IMAGE_TOML)).unwrap();
        assert!(text.starts_with("# A koje image"), "{text}");
        let c = load_config(&dir).unwrap();
        assert_eq!(
            (c.label.as_deref(), c.description.as_deref()),
            (Some("Web"), Some("Node and VS Code"))
        );
        set(&p, &n("web"), Some(""), None).unwrap();
        assert_eq!(load_config(&dir).unwrap().label, None);
        assert!(set(&p, &n("fedora"), Some("x"), None).is_err());
        assert!(set(&p, &n("nope"), Some("x"), None).is_err());

        let kp = ContextName::new("kp").unwrap();
        crate::context::create(&p, &kp, None, None).unwrap();
        crate::context::set_image(&p, &kp, "web").unwrap();
        let err = remove(&p, &n("web")).unwrap_err();
        assert!(err.to_string().contains("used by kp"), "{err}");
        crate::context::set_image(&p, &kp, "fedora").unwrap();
        remove(&p, &n("web")).unwrap();
        assert!(!dir.exists());
        assert!(remove(&p, &n("fedora")).is_err());
    }

    #[test]
    fn set_writes_image_toml_for_a_folder_without_one() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let dir = p.images_dir().join("bare");
        std::fs::create_dir_all(&dir).unwrap();
        set(&p, &n("bare"), Some("Bare"), None).unwrap();
        assert_eq!(load_config(&dir).unwrap().label.as_deref(), Some("Bare"));
    }
}
