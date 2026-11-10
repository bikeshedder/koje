//! A context's image (ADR 0040, 0063): resolved from `image = …`, built or
//! pulled with what it builds on, parents first, and koje's user layer on
//! top. Images whose sources did not change are not built again; a build
//! never touches an existing container.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use koje_core::image::{
    self, ImageRef, LABEL_GBM_DIR, LABEL_MISSING, LABEL_PARENTS, LABEL_SOURCE, LABEL_USER_LAYER,
    Source,
};
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use koje_podman::Podman;
use koje_podman::image::{Build, USER_LAYER, built_in, temp_context};

/// A context's image, ready to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    /// The image with the user layer.
    pub tag: String,
    /// Where its GBM looks for backends.
    pub gbm_dir: String,
    /// Tools the probe did not find.
    pub missing: BTreeSet<String>,
}

/// How [`ensure`] builds.
#[derive(Debug, Clone, Copy, Default)]
#[allow(clippy::struct_excessive_bools)] // independent choices
pub struct Options {
    /// Build every image of the chain, changed or not (`koje image build`).
    pub force: bool,
    pub no_cache: bool,
    /// Pull newer base and registry images.
    pub pull: bool,
}

/// Where `image = value` comes from (its hint, e.g. for the old
/// `koje/base:latest`, said once).
pub fn source(paths: &Paths, value: &str) -> Result<Source> {
    let parsed = image::parse(value)?;
    if let Some(hint) = &parsed.hint {
        crate::say!("note: {hint}");
    }
    resolve(paths, &parsed.image)
}

fn resolve(paths: &Paths, image: &ImageRef) -> Result<Source> {
    let home = paths.host_dirs()?.home;
    Ok(image::resolve(
        image,
        &paths.images_dir(),
        &home,
        Path::is_dir,
    )?)
}

/// The images `src` builds on, resolved.
fn parents(paths: &Paths, src: &Source) -> Result<Vec<Source>> {
    let text = match src {
        Source::Folder { dir, .. } => {
            let file = containerfile(dir)?;
            std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?
        }
        Source::BuiltIn(name) => built_in(name.as_str())
            .and_then(|f| f.iter().find(|(n, _)| *n == "Containerfile"))
            .map(|(_, t)| (*t).to_owned())
            .unwrap_or_default(),
        Source::Registry(_) => return Ok(Vec::new()),
    };
    image::parents(&text)
        .into_iter()
        .map(|name| resolve(paths, &ImageRef::Name(name)))
        .collect()
}

fn containerfile(dir: &Path) -> Result<PathBuf> {
    image::containerfile(dir, Path::is_file)
        .with_context(|| format!("{} has no Containerfile or Dockerfile", dir.display()))
}

/// The hash of a folder's files for `io.koje.source` (`.git` left out).
fn folder_hash(dir: &Path) -> Result<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .collect::<std::io::Result<_>>()?;
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                if entry.file_name() != ".git" {
                    walk(root, &path, out)?;
                }
            } else if kind.is_file() {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                let bytes =
                    std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
                out.push((rel.display().to_string(), bytes));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, dir, &mut files)?;
    Ok(image::source_hash(&files))
}

fn embedded_hash(files: &[(&str, &str)]) -> String {
    let files: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(n, t)| ((*n).to_owned(), t.as_bytes().to_vec()))
        .collect();
    image::source_hash(&files)
}

/// What `src`'s image is built from: the hash of its files.
fn source_hash(src: &Source) -> Result<Option<String>> {
    Ok(match src {
        Source::Folder { dir, .. } => Some(folder_hash(dir)?),
        Source::BuiltIn(name) => built_in(name.as_str()).map(embedded_hash),
        Source::Registry(_) => None,
    })
}

fn say_line(line: &str) {
    crate::say!("  {line}");
}

/// Build or pull what `value` names and what it builds on, as far as
/// missing or changed, then the user layer.
pub fn ensure(paths: &Paths, value: &str, opts: Options) -> Result<Ready> {
    let podman = Podman::new();
    let root = source(paths, value)?;
    let order = image::build_order(&root, |s| {
        parents(paths, s).map_err(|e| koje_core::Error::Image {
            value: s.to_string(),
            reason: format!("{e:#}"),
        })
    })?;
    let mut ids: BTreeMap<Source, String> = BTreeMap::new();
    for src in &order {
        let parent_ids: Vec<String> = parents(paths, src)?
            .iter()
            .filter_map(|p| ids.get(p).cloned())
            .collect();
        let id = base_image(&podman, src, &parent_ids, opts)?;
        ids.insert(src.clone(), id);
    }
    let base_id = ids.get(&root).cloned().context("the image was not built")?;
    user_layer(&podman, &root, &base_id, opts)
}

/// Build (folders, built-in images) or pull (registry images) one image of
/// the chain when needed; its ID.
fn base_image(
    podman: &Podman,
    src: &Source,
    parent_ids: &[String],
    opts: Options,
) -> Result<String> {
    let tag = src.tag();
    if let Source::Registry(reference) = src {
        if opts.pull || !podman.image_exists(reference)? {
            crate::say!("pulling {reference}");
            podman.pull(reference, &mut say_line)?;
        }
        return podman
            .image_id(reference)?
            .with_context(|| format!("{reference} is not there after pulling"));
    }
    let hash = source_hash(src)?.unwrap_or_default();
    let labels = BTreeMap::from([
        (LABEL_SOURCE.to_owned(), hash),
        (LABEL_PARENTS.to_owned(), parent_ids.join(",")),
    ]);
    let current = podman.image_labels(&tag)?;
    let up_to_date = labels.iter().all(|(k, v)| current.get(k) == Some(v));
    if !up_to_date || opts.force || opts.pull {
        let why = if current.is_empty() {
            "not built yet"
        } else if up_to_date {
            "as asked"
        } else {
            "its sources changed"
        };
        crate::say!("building {src} ({why}) – this can take a few minutes");
        let mut b = Build {
            tag: &tag,
            labels,
            no_cache: opts.no_cache,
            pull: opts.pull,
            ..Build::default()
        };
        match src {
            Source::Folder { dir, .. } => {
                let file = containerfile(dir)?;
                b.dir = dir;
                b.file = Some(&file);
                podman.build_image(&b, &mut say_line)?;
            }
            Source::BuiltIn(name) => {
                let files = built_in(name.as_str()).context("no such built-in image")?;
                let temp = temp_context(files)?;
                b.dir = temp.path();
                podman.build_image(&b, &mut say_line)?;
            }
            Source::Registry(_) => unreachable!("pulled above"),
        }
        crate::say!("built {tag}");
    }
    podman
        .image_id(&tag)?
        .with_context(|| format!("{tag} is not there after building"))
}

/// The version of the user layer for this user: its files and the user.
fn layer_version(user: &koje_podman::image::ImageUser) -> String {
    let mut files: Vec<(String, Vec<u8>)> = USER_LAYER
        .iter()
        .map(|(n, t)| ((*n).to_owned(), t.as_bytes().to_vec()))
        .collect();
    for (k, v) in user.build_args() {
        files.push((k, v.into_bytes()));
    }
    image::source_hash(&files)
}

/// koje's user layer on `base` (ID `base_id`), built when missing or when
/// its base or koje's layer changed. Refuses images no context can run on.
fn user_layer(podman: &Podman, src: &Source, base_id: &str, opts: Options) -> Result<Ready> {
    let tag = src.user_tag();
    let user = koje_podman::host::image_user().context("finding the current user")?;
    let version = layer_version(&user);
    let current = podman.image_labels(&tag)?;
    let ready = |labels: &BTreeMap<String, String>| Ready {
        tag: tag.clone(),
        gbm_dir: labels
            .get(LABEL_GBM_DIR)
            .cloned()
            .unwrap_or_else(|| koje_core::spec::DEFAULT_GBM_DIR.to_owned()),
        missing: labels
            .get(LABEL_MISSING)
            .map(|m| {
                m.split(',')
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    };
    if !opts.force
        && current.get(LABEL_PARENTS).map(String::as_str) == Some(base_id)
        && current.get(LABEL_USER_LAYER) == Some(&version)
    {
        return Ok(ready(&current));
    }
    let base_tag = src.tag();
    let probe = image::read_probe(
        &podman
            .probe(&base_tag, image::PROBE)
            .with_context(|| format!("looking into {base_tag}"))?,
    );
    let blockers = image::blockers(&probe.missing);
    if !blockers.is_empty() {
        bail!("{src} cannot be a context's image: {}", blockers.join("; "));
    }
    let gbm = probe
        .gbm_dir
        .unwrap_or_else(|| koje_core::spec::DEFAULT_GBM_DIR.to_owned());
    let missing: Vec<&str> = probe.missing.iter().map(String::as_str).collect();
    let labels = BTreeMap::from([
        (LABEL_PARENTS.to_owned(), base_id.to_owned()),
        (LABEL_USER_LAYER.to_owned(), version),
        (LABEL_GBM_DIR.to_owned(), gbm),
        (LABEL_MISSING.to_owned(), missing.join(",")),
    ]);
    let mut args = user.build_args();
    args.insert("KOJE_IMAGE".to_owned(), base_tag);
    let temp = temp_context(USER_LAYER)?;
    crate::say!("adding your user to {src}");
    podman.build_image(
        &Build {
            tag: &tag,
            dir: temp.path(),
            labels: labels.clone(),
            args,
            no_cache: opts.no_cache,
            local_from: true,
            ..Build::default()
        },
        &mut say_line,
    )?;
    Ok(ready(&labels))
}

/// Why `ctx`'s container no longer runs its image as it is now: the image
/// was rebuilt, or an image folder it is built from changed. `None` when it
/// is current or there is no container. Builds nothing.
pub fn outdated(paths: &Paths, ctx: &ContextName, value: &str) -> Result<Option<String>> {
    let podman = Podman::new();
    let Some(running) = podman.container_image_id(ctx)? else {
        return Ok(None);
    };
    let root = resolve(paths, &image::parse(value)?.image)?;
    let mut seen = BTreeSet::new();
    let mut todo = vec![root.clone()];
    while let Some(src) = todo.pop() {
        if !seen.insert(src.clone()) {
            continue;
        }
        if let Source::Folder { dir, .. } = &src {
            let labels = podman.image_labels(&src.tag())?;
            if labels.get(LABEL_SOURCE) != Some(&folder_hash(dir)?) {
                return Ok(Some(format!("{} changed", dir.display())));
            }
        }
        todo.extend(parents(paths, &src)?);
    }
    Ok(match podman.image_id(&root.user_tag())? {
        Some(id) if id == running => None,
        Some(_) => Some(format!("{root} was rebuilt")),
        None => Some(format!("{root} is not built yet")),
    })
}

/// The images contexts use, each once (`koje image build` without names).
pub fn in_use(paths: &Paths) -> Result<Vec<String>> {
    let mut out = BTreeSet::new();
    for cfg in koje_core::context::list(paths)? {
        out.insert(cfg.image);
    }
    Ok(out.into_iter().collect())
}

/// The Containerfile of the image folder `value` names; `None` for koje's
/// and registry images.
pub fn folder_file(paths: &Paths, value: &str) -> Result<Option<PathBuf>> {
    let src = resolve(paths, &image::parse(value)?.image)?;
    Ok(match src {
        Source::Folder { dir, .. } => Some(containerfile(&dir)?),
        _ => None,
    })
}

/// A context's own Containerfile (ADR 0063).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnFile {
    pub file: PathBuf,
    /// The image folder was made now.
    pub created: bool,
    /// `image = …` was changed to it now.
    pub image_set: bool,
}

/// The Containerfile of `ctx`'s image folder: the one it uses, else its own
/// (`images/<context>/`), made `FROM` its current image when missing and set
/// as its image.
pub fn own_file(paths: &Paths, ctx: &ContextName) -> Result<OwnFile> {
    let cfg = koje_core::context::load(paths, ctx)?;
    if let Source::Folder { dir, .. } = source(paths, &cfg.image)? {
        return Ok(OwnFile {
            file: containerfile(&dir)?,
            created: false,
            image_set: false,
        });
    }
    let name = image::own_image(ctx)?;
    let dir = paths.images_dir().join(name.as_str());
    let created = !dir.exists();
    let file = if created {
        let text = new_containerfile(&from_line(paths, &cfg.image)?);
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let file = dir.join("Containerfile");
        koje_core::fsutil::write_atomic(&file, text.as_bytes())?;
        file
    } else {
        // Made before (another context's, or by hand): used as it is.
        containerfile(&dir)?
    };
    koje_core::context::set_image(paths, ctx, name.as_str())?;
    Ok(OwnFile {
        file,
        created,
        image_set: true,
    })
}

/// The `Containerfile` of a new image folder that builds on `from`.
pub fn new_containerfile(from: &str) -> String {
    format!(
        "# A koje image (ADR 0040): built by koje when a context that uses it is\n\
         # created (koje up, koje recreate), with your user added on top.\n\
         # Install programs here so they survive a recreate, e.g.\n\
         #   RUN dnf -y install code && dnf clean all\n\
         # Files next to this one can be copied in with COPY.\n\
         FROM {from}\n"
    )
}

/// What a new image folder writes after `FROM` for a context's image: its
/// image as Podman knows it.
pub fn from_line(paths: &Paths, value: &str) -> Result<String> {
    Ok(match source(paths, value)? {
        Source::Registry(reference) => reference,
        other => other.tag(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_hash_follows_the_files_and_skips_git() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("Containerfile"), "FROM x\n").unwrap();
        let first = folder_hash(dir).unwrap();
        std::fs::create_dir(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".git/HEAD"), "ref").unwrap();
        assert_eq!(folder_hash(dir).unwrap(), first);
        std::fs::create_dir(dir.join("conf")).unwrap();
        std::fs::write(dir.join("conf/a"), "1").unwrap();
        let second = folder_hash(dir).unwrap();
        assert_ne!(second, first);
        std::fs::write(dir.join("conf/a"), "2").unwrap();
        assert_ne!(folder_hash(dir).unwrap(), second);
    }

    fn paths(tmp: &Path) -> Paths {
        Paths::with_roots(tmp.join("c"), tmp.join("d"), tmp.join("s"))
    }

    #[test]
    fn a_context_gets_its_own_containerfile_once() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let kp = ContextName::new("kp").unwrap();
        koje_core::context::create(&p, &kp, None, None).unwrap();

        let own = own_file(&p, &kp).unwrap();
        assert_eq!(own.file, p.images_dir().join("kp/Containerfile"));
        assert!(own.created && own.image_set);
        let text = std::fs::read_to_string(&own.file).unwrap();
        assert!(
            text.ends_with("FROM localhost/koje/fedora:latest\n"),
            "{text}"
        );
        assert_eq!(koje_core::context::load(&p, &kp).unwrap().image, "kp");

        // Edited, then asked again: the same file, nothing changed.
        std::fs::write(&own.file, "FROM localhost/koje/fedora\nRUN true\n").unwrap();
        let again = own_file(&p, &kp).unwrap();
        assert_eq!(again.file, own.file);
        assert!(!again.created && !again.image_set);
        assert!(
            std::fs::read_to_string(&own.file)
                .unwrap()
                .contains("RUN true")
        );
    }

    #[test]
    fn a_context_on_a_shared_folder_edits_that_one() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let kp = ContextName::new("kp").unwrap();
        koje_core::context::create(&p, &kp, None, None).unwrap();
        let dev = p.images_dir().join("dev");
        std::fs::create_dir_all(&dev).unwrap();
        std::fs::write(dev.join("Dockerfile"), "FROM localhost/koje/fedora\n").unwrap();
        koje_core::context::set_image(&p, &kp, "dev").unwrap();

        let own = own_file(&p, &kp).unwrap();
        assert_eq!(own.file, dev.join("Dockerfile"));
        assert!(!own.created && !own.image_set);
        assert!(!p.images_dir().join("kp").exists());
    }

    #[test]
    fn a_registry_image_is_kept_as_the_base() {
        let tmp = tempfile::tempdir().unwrap();
        let p = paths(tmp.path());
        let kp = ContextName::new("kp").unwrap();
        koje_core::context::create(&p, &kp, None, None).unwrap();
        koje_core::context::set_image(&p, &kp, "docker.io/library/debian:13").unwrap();
        let own = own_file(&p, &kp).unwrap();
        let text = std::fs::read_to_string(&own.file).unwrap();
        assert!(
            text.ends_with("FROM docker.io/library/debian:13\n"),
            "{text}"
        );
    }

    #[test]
    fn new_containerfile_builds_on_the_image() {
        let text = new_containerfile("localhost/koje/fedora:latest");
        assert!(
            text.ends_with("FROM localhost/koje/fedora:latest\n"),
            "{text}"
        );
        assert_eq!(
            image::parents(&text),
            [koje_core::name::ImageName::new("fedora").unwrap()]
        );
    }
}
