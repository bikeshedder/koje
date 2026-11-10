//! Images as koje keeps and lists them (ADR 0064): koje's built-in images
//! and the image folders in `~/.config/koje/images/<name>/`, each with its
//! `Containerfile` and an optional `image.toml` (label, description), and
//! the contexts that use them. Like [`crate::context`] for contexts.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::fsutil;
use crate::image::{self, ImageRef, Source, is_built_in};
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
    /// The image's version (ADR 0067): its tag, `localhost/koje/<name>:<version>`.
    /// Unset: `1`. A customized koje image has koje's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// A customized built-in image (ADR 0065): the hash of koje's files it
    /// was copied from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_koje: Option<String>,
}

/// What koje says about its built-in images.
const BUILT_IN_TEXT: &[(&str, &str, &str)] = &[
    (
        "fedora",
        "Fedora 45",
        "koje's image: Fedora with Firefox, foot, Nautilus, Podman and fonts",
    ),
    (
        "ubuntu",
        "Ubuntu 26.04 LTS",
        "koje's image: Ubuntu with Firefox, foot, Nautilus, Podman and fonts",
    ),
    (
        "debian",
        "Debian 13",
        "koje's image: Debian with Firefox ESR, foot, Nautilus, Podman and fonts",
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    BuiltIn,
    Folder,
    /// A folder in a built-in image's place (ADR 0065).
    Customized,
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
    /// Its version: koje's release, or the folder's (ADR 0067).
    pub version: String,
    /// Contexts whose `image = …` names it.
    pub used_by: Vec<ContextName>,
    /// Why its `image.toml` or Containerfile is not usable.
    pub error: Option<String>,
    /// A customized image: the version of koje's files it was copied from.
    pub from_koje: Option<String>,
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
            .filter(|c| image::parse(&c.image).is_ok_and(|p| p.image.koje_name() == Some(name)))
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
                version: image::built_in_version(name.as_str())
                    .unwrap_or(image::FIRST_VERSION)
                    .to_owned(),
                name,
                kind: Kind::BuiltIn,
                label: (*label).to_owned(),
                description: (*description).to_owned(),
                dir: None,
                error: None,
                from_koje: None,
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
        let built_in = BUILT_IN_TEXT.iter().find(|(n, _, _)| *n == name.as_str());
        out.push(Entry {
            used_by: used_by(&name),
            label: config
                .label
                .or_else(|| built_in.map(|(_, l, _)| (*l).to_owned()))
                .unwrap_or_else(|| name.to_string()),
            description: config.description.unwrap_or_default(),
            kind: if built_in.is_some() {
                Kind::Customized
            } else {
                Kind::Folder
            },
            version: image::built_in_version(name.as_str()).map_or_else(
                || {
                    config
                        .version
                        .clone()
                        .unwrap_or_else(|| image::FIRST_VERSION.to_owned())
                },
                str::to_owned,
            ),
            name,
            dir: Some(dir),
            error,
            from_koje: config.from_koje,
        });
    }
    Ok(out)
}

impl Entry {
    /// What `image = …` and `FROM` name: `localhost/koje/<name>:<version>`.
    pub fn reference(&self) -> String {
        image::reference(self.name.as_str(), &self.version)
    }
}

/// An image folder's version: `image.toml`'s, else `1`.
pub fn folder_version(dir: &Path) -> String {
    load_config(dir)
        .ok()
        .and_then(|c| c.version)
        .unwrap_or_else(|| image::FIRST_VERSION.to_owned())
}

/// Where `image` comes from (ADR 0040, 0067): koje's images, the folders,
/// paths and registries; `home` expands `~/`.
pub fn resolve(paths: &Paths, home: &Path, image: &ImageRef) -> Result<Source> {
    image::resolve(
        image,
        &paths.images_dir(),
        home,
        Path::is_dir,
        folder_version,
    )
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
    if is_built_in(name.as_str()) {
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
        version: Some(image::FIRST_VERSION.to_owned()),
        ..ImageConfig::default()
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
         # description = \"…\"\n\
         # version = \"1\"      its tag: localhost/koje/<name>:<version>\n\
         # from_koje = \"…\"    a customized koje image: the version it was copied from\n{body}"
    );
    fsutil::write_atomic(&dir.join(IMAGE_TOML), text.as_bytes())
}

/// The folder of image `name`; refuses built-in images and missing folders.
fn folder(paths: &Paths, name: &ImageName) -> Result<PathBuf> {
    let dir = paths.images_dir().join(name.as_str());
    if dir.is_dir() {
        Ok(dir)
    } else if is_built_in(name.as_str()) {
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

/// Customize built-in image `name` (ADR 0065): its `files` (koje's, with
/// the hash `version`) copied into its folder, which takes its place.
pub fn customize(
    paths: &Paths,
    name: &ImageName,
    files: &[(&str, &str)],
    version: &str,
) -> Result<PathBuf> {
    let Some((_, label, description)) = BUILT_IN_TEXT.iter().find(|(n, _, _)| *n == name.as_str())
    else {
        return Err(Error::Image {
            value: name.to_string(),
            reason: "is not one of koje's own images – edit it as it is".into(),
        });
    };
    let dir = paths.images_dir().join(name.as_str());
    if dir.exists() {
        return Err(Error::Image {
            value: name.to_string(),
            reason: format!("is customized already: {}", dir.display()),
        });
    }
    for (file, text) in files {
        fsutil::write_atomic(&dir.join(file), text.as_bytes())?;
    }
    write_config(
        &dir,
        &ImageConfig {
            label: Some((*label).to_owned()),
            description: Some(format!("{description} (customized)")),
            version: None,
            from_koje: Some(version.to_owned()),
        },
    )?;
    Ok(dir)
}

/// Back to koje's own image (ADR 0065): the customized folder goes; also
/// while contexts use it, since koje's image takes its place.
pub fn reset(paths: &Paths, name: &ImageName) -> Result<()> {
    let dir = paths.images_dir().join(name.as_str());
    if !is_built_in(name.as_str()) || !dir.is_dir() {
        return Err(Error::Image {
            value: name.to_string(),
            reason: "is no customized koje image".into(),
        });
    }
    std::fs::remove_dir_all(&dir).map_err(|source| Error::Write { path: dir, source })
}

/// Keep a customized image over koje's newer `version`: the mark goes.
pub fn keep(paths: &Paths, name: &ImageName, version: &str) -> Result<()> {
    if !is_built_in(name.as_str()) {
        return Err(Error::Image {
            value: name.to_string(),
            reason: "is no customized koje image".into(),
        });
    }
    let dir = folder(paths, name)?;
    let path = dir.join(IMAGE_TOML);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| Error::InvalidEdit(e.to_string()))?;
    doc["from_koje"] = toml_edit::value(version);
    fsutil::write_atomic(&path, doc.to_string().as_bytes())
}

/// Point everything that names koje image `old` (any version) at `new`
/// (a reference): the contexts' `image = …` and the `FROM` lines of the
/// other image folders (ADR 0067). The contexts changed.
fn move_references(paths: &Paths, old: &ImageName, new: &str) -> Result<Vec<ContextName>> {
    let mut moved = Vec::new();
    for cfg in crate::context::list(paths)? {
        if image::parse(&cfg.image).is_ok_and(|p| p.image.koje_name() == Some(old)) {
            crate::context::set_image(paths, &cfg.name, new)?;
            moved.push(cfg.name);
        }
    }
    for name in folders(paths)? {
        let dir = paths.images_dir().join(name.as_str());
        if let Some(file) = image::containerfile(&dir, Path::is_file) {
            let text = std::fs::read_to_string(&file).map_err(|source| Error::Read {
                path: file.clone(),
                source,
            })?;
            let changed = image::with_from(&text, old, new);
            if changed != text {
                fsutil::write_atomic(&file, changed.as_bytes())?;
            }
        }
    }
    Ok(moved)
}

/// Rename an image folder (ADR 0067); contexts and `FROM` lines follow.
/// The contexts that use it now under its new name.
pub fn rename(paths: &Paths, old: &ImageName, new: &ImageName) -> Result<Vec<ContextName>> {
    if is_built_in(old.as_str()) {
        return Err(Error::Image {
            value: old.to_string(),
            reason: "is koje's own image and keeps its name".into(),
        });
    }
    if is_built_in(new.as_str()) {
        return Err(Error::Image {
            value: new.to_string(),
            reason: "is koje's own image – choose another name".into(),
        });
    }
    let from = folder(paths, old)?;
    let to = paths.images_dir().join(new.as_str());
    if to.exists() {
        return Err(Error::Image {
            value: new.to_string(),
            reason: format!("{} exists already", to.display()),
        });
    }
    std::fs::rename(&from, &to).map_err(|source| Error::Write {
        path: to.clone(),
        source,
    })?;
    move_references(
        paths,
        old,
        &image::reference(new.as_str(), &folder_version(&to)),
    )
}

/// Give an image folder another version (ADR 0067); contexts and `FROM`
/// lines follow. The contexts that use it.
pub fn set_version(paths: &Paths, name: &ImageName, version: &str) -> Result<Vec<ContextName>> {
    image::check_version(version)?;
    if is_built_in(name.as_str()) {
        return Err(Error::Image {
            value: name.to_string(),
            reason: "is koje's own image and has koje's version".into(),
        });
    }
    let dir = folder(paths, name)?;
    let path = dir.join(IMAGE_TOML);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| Error::InvalidEdit(e.to_string()))?;
    doc["version"] = toml_edit::value(version);
    fsutil::write_atomic(&path, doc.to_string().as_bytes())?;
    move_references(paths, name, &image::reference(name.as_str(), version))
}

/// Remove an image folder; refused while a context uses it.
pub fn remove(paths: &Paths, name: &ImageName) -> Result<()> {
    if is_built_in(name.as_str()) {
        return Err(Error::Image {
            value: name.to_string(),
            reason: "is koje's own image – a customized one is reset instead".into(),
        });
    }
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
        crate::context::create(&p, &kp, None, None, "localhost/koje/fedora:45").unwrap();
        crate::context::create(&p, &shop, None, None, "localhost/koje/fedora:45").unwrap();
        create(
            &p,
            &n("web"),
            "FROM localhost/koje/fedora\n",
            Some("Web development"),
        )
        .unwrap();
        crate::context::set_image(&p, &shop, "localhost/koje/web:1").unwrap();
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
        assert_eq!(
            names,
            ["fedora", "ubuntu", "debian", "bare", "broken", "web"]
        );
        let fedora = &all[0];
        assert_eq!(
            (fedora.kind, fedora.label.as_str()),
            (Kind::BuiltIn, "Fedora 45")
        );
        assert_eq!(fedora.reference(), "localhost/koje/fedora:45");
        assert_eq!(fedora.used_by, [kp]);
        assert_eq!(all[1].reference(), "localhost/koje/ubuntu:26.04");
        assert_eq!(all[2].reference(), "localhost/koje/debian:13");
        let web = &all[5];
        assert_eq!(web.label, "Web development");
        assert_eq!(web.reference(), "localhost/koje/web:1");
        assert_eq!(web.used_by, [shop]);
        assert_eq!(
            web.dir.as_deref(),
            Some(p.images_dir().join("web").as_path())
        );
        assert_eq!(all[3].label, "bare");
        assert_eq!(all[3].error, None);
        assert_eq!(all[3].version, "1");
        assert!(
            all[4].error.as_deref().unwrap().contains("colour"),
            "{:?}",
            all[4].error
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
        assert_eq!(all.len(), 3);
        assert_eq!(all[2].name.as_str(), "fedora");
        assert_eq!(
            (all[2].kind, all[2].version.as_str()),
            (Kind::Customized, "45")
        );
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
        crate::context::create(&p, &kp, None, None, "localhost/koje/fedora:45").unwrap();
        crate::context::set_image(&p, &kp, "localhost/koje/web:1").unwrap();
        let err = remove(&p, &n("web")).unwrap_err();
        assert!(err.to_string().contains("used by kp"), "{err}");
        crate::context::set_image(&p, &kp, "localhost/koje/fedora:45").unwrap();
        remove(&p, &n("web")).unwrap();
        assert!(!dir.exists());
        assert!(remove(&p, &n("fedora")).is_err());
    }

    #[test]
    fn a_built_in_image_is_customized_kept_and_reset() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let kp = ContextName::new("kp").unwrap();
        crate::context::create(&p, &kp, None, None, "localhost/koje/fedora:45").unwrap();
        let files = [(
            "Containerfile",
            "FROM registry.fedoraproject.org/fedora:44\n",
        )];
        assert!(customize(&p, &n("web"), &files, "v1").is_err());

        let dir = customize(&p, &n("fedora"), &files, "v1").unwrap();
        assert!(customize(&p, &n("fedora"), &files, "v1").is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("Containerfile")).unwrap(),
            files[0].1
        );
        let all = list(&p).unwrap();
        assert_eq!(all.len(), 3);
        let fedora = all.iter().find(|e| e.name.as_str() == "fedora").unwrap();
        assert_eq!(fedora.kind, Kind::Customized);
        assert_eq!(fedora.label, "Fedora 45");
        assert_eq!(fedora.version, "45");
        assert_eq!(fedora.from_koje.as_deref(), Some("v1"));
        assert_eq!(fedora.used_by, [kp]);
        // Edited like any image; kept over a newer version; never removed.
        set(&p, &n("fedora"), Some("My Fedora"), None).unwrap();
        keep(&p, &n("fedora"), "v2").unwrap();
        let fedora = list(&p)
            .unwrap()
            .into_iter()
            .find(|e| e.name.as_str() == "fedora")
            .unwrap();
        assert_eq!(
            (fedora.label.as_str(), fedora.from_koje.as_deref()),
            ("My Fedora", Some("v2"))
        );
        assert!(remove(&p, &n("fedora")).is_err());
        // Reset while kp uses it: koje's image is back.
        reset(&p, &n("fedora")).unwrap();
        assert!(!dir.exists());
        assert_eq!(list(&p).unwrap()[0].kind, Kind::BuiltIn);
        assert!(reset(&p, &n("fedora")).is_err());
    }

    #[test]
    fn renames_and_new_versions_move_contexts_and_from_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let kp = ContextName::new("kp").unwrap();
        let shop = ContextName::new("shop").unwrap();
        crate::context::create(&p, &kp, None, None, "localhost/koje/web:1").unwrap();
        crate::context::create(&p, &shop, None, None, "localhost/koje/fedora:45").unwrap();
        create(&p, &n("web"), "FROM localhost/koje/fedora:45\n", None).unwrap();
        create(
            &p,
            &n("app"),
            "FROM localhost/koje/web:1 AS base\nRUN true\n",
            None,
        )
        .unwrap();

        let moved = rename(&p, &n("web"), &n("site")).unwrap();
        assert_eq!(moved, std::slice::from_ref(&kp));
        assert!(!p.images_dir().join("web").exists());
        assert_eq!(
            crate::context::load(&p, &kp).unwrap().image,
            "localhost/koje/site:1"
        );
        assert_eq!(
            crate::context::load(&p, &shop).unwrap().image,
            "localhost/koje/fedora:45",
            "others stay"
        );
        let app = p.images_dir().join("app/Containerfile");
        assert_eq!(
            std::fs::read_to_string(&app).unwrap(),
            "FROM localhost/koje/site:1 AS base\nRUN true\n"
        );

        set_version(&p, &n("site"), "2").unwrap();
        assert_eq!(
            crate::context::load(&p, &kp).unwrap().image,
            "localhost/koje/site:2"
        );
        assert!(
            std::fs::read_to_string(&app)
                .unwrap()
                .starts_with("FROM localhost/koje/site:2 ")
        );
        assert_eq!(folder_version(&p.images_dir().join("site")), "2");
        assert!(set_version(&p, &n("site"), "latest").is_err());

        assert!(rename(&p, &n("site"), &n("app")).is_err(), "taken");
        assert!(rename(&p, &n("site"), &n("debian")).is_err(), "koje's name");
        assert!(rename(&p, &n("fedora"), &n("x")).is_err(), "koje's image");
        assert!(set_version(&p, &n("fedora"), "46").is_err());
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
