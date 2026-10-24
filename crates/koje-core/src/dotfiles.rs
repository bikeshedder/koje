//! Dotfiles presets (ADR 0038): folders in `~/.config/koje/dotfiles/<name>`,
//! mounted read-only into a context at `~/.dotfiles/<name>` and linked into
//! its home like GNU Stow does – a folder only one preset provides becomes
//! one link, folders several provide (or the home has already) become real
//! folders with links inside. Presets later in `context.toml` win. Top-level
//! folders (`.config`, `.local`) always stay real: programs write their own
//! files there, and the presets are read-only.
//!
//! Pure except [`scan`], which reads a preset folder (koje's own config). The
//! CLI probes the context's home with [`PROBE_SCRIPT`], plans with [`plan`]
//! and runs the steps with [`APPLY_SCRIPT`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::name::PresetName;

/// Where presets are mounted, below the context's home.
pub const DIR: &str = ".dotfiles";

/// Where a preset is mounted, below the context's home.
pub fn mount_rel(name: &PresetName) -> PathBuf {
    Path::new(DIR).join(name.as_str())
}

/// Something in a preset, as a path below its folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub rel: PathBuf,
    pub dir: bool,
}

/// A preset's entries, top-down. A top-level `.git` is skipped (a preset may
/// be a repository); symbolic links count as files (linked, not entered);
/// names with a tab or newline are skipped (the probe is line based).
pub fn scan(dir: &Path) -> Result<Vec<Entry>> {
    fn walk(root: &Path, rel: &Path, out: &mut Vec<Entry>) -> Result<()> {
        let path = root.join(rel);
        let read = std::fs::read_dir(&path).map_err(|source| Error::Read {
            path: path.clone(),
            source,
        })?;
        let mut children: Vec<(String, bool)> = read
            .filter_map(std::result::Result::ok)
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let dir = e.file_type().ok()?.is_dir();
                Some((name, dir))
            })
            .filter(|(name, _)| !name.contains(['\t', '\n']))
            .filter(|(name, _)| !(rel.as_os_str().is_empty() && name == ".git"))
            .collect();
        children.sort();
        for (name, dir) in children {
            let child = rel.join(&name);
            out.push(Entry {
                rel: child.clone(),
                dir,
            });
            if dir {
                walk(root, &child, out)?;
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, Path::new(""), &mut out)?;
    Ok(out)
}

/// What a path in the context's home is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    Missing,
    Dir,
    File,
    Link(PathBuf),
}

/// Prints `L\t<path>\t<target>`, `D\t<path>` or `F\t<path>` for every
/// argument (a path below `$HOME`) that exists; for `sh -c`.
pub const PROBE_SCRIPT: &str = r#"for p; do f="$HOME/$p"
if [ -L "$f" ]; then printf 'L\t%s\t%s\n' "$p" "$(readlink "$f")"
elif [ -d "$f" ]; then printf 'D\t%s\n' "$p"
elif [ -e "$f" ]; then printf 'F\t%s\n' "$p"; fi; done"#;

/// The output of [`PROBE_SCRIPT`]; missing paths are not in it.
pub fn parse_probe(out: &str) -> BTreeMap<PathBuf, Found> {
    out.lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, '\t');
            let kind = parts.next()?;
            let path = PathBuf::from(parts.next()?);
            let found = match kind {
                "L" => Found::Link(PathBuf::from(parts.next()?)),
                "D" => Found::Dir,
                "F" => Found::File,
                _ => return None,
            };
            Some((path, found))
        })
        .collect()
}

/// One change in the context's home, paths below it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Remove a link of koje's.
    Remove(PathBuf),
    MakeDir(PathBuf),
    /// Create or replace a link.
    Link {
        path: PathBuf,
        target: PathBuf,
    },
}

/// Runs steps given as arguments: `rm <path>`, `mkdir <path>`, `ln <path>
/// <target>` (paths below `$HOME`); `rm` removes only links. Prints each
/// failure and goes on; for `sh -c`.
pub const APPLY_SCRIPT: &str = r#"while [ $# -gt 0 ]; do f="$HOME/$2"
case $1 in
rm) [ -L "$f" ] && rm -f "$f"; shift 2 ;;
mkdir) mkdir -p "$f" || echo "mkdir $2 failed"; shift 2 ;;
ln) mkdir -p "$(dirname "$f")" && ln -sfn "$3" "$f" || echo "ln $2 failed"; shift 3 ;;
*) echo "unknown step $1"; exit 1 ;;
esac; done; true"#;

/// The arguments for [`APPLY_SCRIPT`].
pub fn apply_args(steps: &[Step]) -> Vec<String> {
    let s = |p: &Path| p.display().to_string();
    steps
        .iter()
        .flat_map(|step| match step {
            Step::Remove(p) => vec!["rm".to_owned(), s(p)],
            Step::MakeDir(p) => vec!["mkdir".to_owned(), s(p)],
            Step::Link { path, target } => vec!["ln".to_owned(), s(path), s(target)],
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// Every link the presets want, made now or there already – the next
    /// run removes those of them no longer wanted.
    pub links: Vec<PathBuf>,
    /// Paths where the home has something of its own: kept, not linked.
    pub conflicts: Vec<PathBuf>,
}

/// The paths to probe in the context's home before [`plan`]: everything the
/// presets have, and the links made last time.
pub fn probe_paths(presets: &[(PresetName, Vec<Entry>)], previous: &[PathBuf]) -> Vec<PathBuf> {
    let mut all: BTreeSet<PathBuf> = presets
        .iter()
        .flat_map(|(_, entries)| entries.iter().map(|e| e.rel.clone()))
        .collect();
    all.extend(previous.iter().cloned());
    all.into_iter().collect()
}

/// What [`visit`] needs from [`plan`].
struct Ctx<'a> {
    base: &'a Path,
    providers: &'a BTreeMap<&'a Path, Vec<(&'a PresetName, bool)>>,
    found: &'a BTreeMap<PathBuf, Found>,
}

/// The children of `dir` that presets provide; `fresh`: `dir` was just
/// made (nothing is in it).
fn visit(c: &Ctx<'_>, dir: &Path, fresh: bool, out: &mut Plan) {
    let ours = |f: &Found| matches!(f, Found::Link(t) if t.starts_with(c.base));
    let children: Vec<&Path> = c
        .providers
        .keys()
        .copied()
        .filter(|p| p.parent() == Some(dir))
        .collect();
    for rel in children {
        let who = &c.providers[rel];
        let Some(&(last, last_dir)) = who.last() else {
            continue;
        };
        // A folder created just now (or replacing a link) is empty: what a
        // probe saw there went through the old link.
        let found = if fresh {
            Found::Missing
        } else {
            c.found.get(rel).cloned().unwrap_or(Found::Missing)
        };
        let target = c.base.join(last.as_str()).join(rel);
        // Folders after the last file win together; a file hides earlier ones.
        let folders = who.iter().rev().take_while(|(_, d)| *d).count();
        let top_level = rel.parent() == Some(Path::new(""));
        if !last_dir || (folders == 1 && !top_level && !matches!(found, Found::Dir)) {
            match &found {
                Found::Missing => out.steps.push(Step::Link {
                    path: rel.to_path_buf(),
                    target,
                }),
                Found::Link(t) if *t == target => {}
                f if ours(f) => out.steps.push(Step::Link {
                    path: rel.to_path_buf(),
                    target,
                }),
                _ => {
                    out.conflicts.push(rel.to_path_buf());
                    continue;
                }
            }
            out.links.push(rel.to_path_buf());
            continue;
        }
        // A real folder with links inside.
        match &found {
            Found::Dir => visit(c, rel, false, out),
            Found::Missing => {
                out.steps.push(Step::MakeDir(rel.to_path_buf()));
                visit(c, rel, true, out);
            }
            f if ours(f) => {
                out.steps.push(Step::Remove(rel.to_path_buf()));
                out.steps.push(Step::MakeDir(rel.to_path_buf()));
                visit(c, rel, true, out);
            }
            _ => out.conflicts.push(rel.to_path_buf()),
        }
    }
}

/// What to change so the context's home links to its presets. `home`: the
/// context's home (link targets are absolute); `presets` in `context.toml`
/// order; `found`: the probe; `previous`: the links made last time.
pub fn plan(
    home: &Path,
    presets: &[(PresetName, Vec<Entry>)],
    found: &BTreeMap<PathBuf, Found>,
    previous: &[PathBuf],
) -> Plan {
    let base = home.join(DIR);
    // Who provides each path, in preset order: (preset, is a folder).
    let mut providers: BTreeMap<&Path, Vec<(&PresetName, bool)>> = BTreeMap::new();
    for (name, entries) in presets {
        for e in entries {
            providers.entry(&e.rel).or_default().push((name, e.dir));
        }
    }
    let ours = |f: &Found| matches!(f, Found::Link(t) if t.starts_with(&base));
    let mut out = Plan::default();

    // Links of koje's from last time that no preset wants any more.
    let wanted_somewhere: BTreeSet<&Path> = providers.keys().copied().collect();
    for p in previous {
        if !wanted_somewhere.contains(p.as_path()) && found.get(p).is_some_and(ours) {
            out.steps.push(Step::Remove(p.clone()));
        }
    }

    let c = Ctx {
        base: &base,
        providers: &providers,
        found,
    };
    visit(&c, Path::new(""), false, &mut out);
    out
}

/// The links file (one path per line).
pub fn parse_links(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

pub fn links_text(links: &[PathBuf]) -> String {
    use std::fmt::Write as _;
    let mut text = String::new();
    for l in links {
        let _ = writeln!(text, "{}", l.display());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(s: &str) -> PresetName {
        PresetName::new(s).unwrap()
    }

    fn entries(list: &[(&str, bool)]) -> Vec<Entry> {
        list.iter()
            .map(|(p, dir)| Entry {
                rel: PathBuf::from(p),
                dir: *dir,
            })
            .collect()
    }

    fn link(path: &str, target: &str) -> Step {
        Step::Link {
            path: path.into(),
            target: target.into(),
        }
    }

    const HOME: &str = "/home/mj";

    fn found(list: &[(&str, Found)]) -> BTreeMap<PathBuf, Found> {
        list.iter()
            .map(|(p, f)| (PathBuf::from(p), f.clone()))
            .collect()
    }

    fn base() -> Vec<(PresetName, Vec<Entry>)> {
        vec![(
            name("base"),
            entries(&[
                (".bashrc", false),
                (".config", true),
                (".config/foot", true),
                (".config/foot/foot.ini", false),
            ]),
        )]
    }

    #[test]
    fn a_fresh_home() {
        // ~/.config exists in every home; foot's folder becomes one link.
        let p = plan(
            Path::new(HOME),
            &base(),
            &found(&[(".config", Found::Dir)]),
            &[],
        );
        assert_eq!(
            p.steps,
            [
                link(".bashrc", "/home/mj/.dotfiles/base/.bashrc"),
                link(".config/foot", "/home/mj/.dotfiles/base/.config/foot"),
            ]
        );
        assert_eq!(p.links, [PathBuf::from(".bashrc"), ".config/foot".into()]);
        assert_eq!(p.conflicts, Vec::<PathBuf>::new());
    }

    #[test]
    fn top_level_folders_stay_real() {
        let p = plan(Path::new(HOME), &base(), &BTreeMap::new(), &[]);
        assert_eq!(
            p.steps[1..],
            [
                Step::MakeDir(".config".into()),
                link(".config/foot", "/home/mj/.dotfiles/base/.config/foot"),
            ]
        );
    }

    #[test]
    fn done_already_and_conflicts() {
        let p = plan(
            Path::new(HOME),
            &base(),
            &found(&[
                (".bashrc", Found::File),
                (".config", Found::Dir),
                (
                    ".config/foot",
                    Found::Link("/home/mj/.dotfiles/base/.config/foot".into()),
                ),
            ]),
            &[],
        );
        assert_eq!(p.steps, [], "the user's .bashrc stays, foot is linked");
        assert_eq!(p.conflicts, [PathBuf::from(".bashrc")]);
        assert_eq!(p.links, [PathBuf::from(".config/foot")]);
    }

    #[test]
    fn a_real_folder_in_the_home_is_entered() {
        // The context has its own ~/.config/foot (e.g. a theme file): link
        // inside it instead.
        let p = plan(
            Path::new(HOME),
            &base(),
            &found(&[(".config", Found::Dir), (".config/foot", Found::Dir)]),
            &[],
        );
        assert_eq!(
            p.steps[1..],
            [link(
                ".config/foot/foot.ini",
                "/home/mj/.dotfiles/base/.config/foot/foot.ini"
            )]
        );
    }

    #[test]
    fn layered_presets() {
        let mut presets = base();
        presets.push((
            name("dev"),
            entries(&[
                (".bashrc", false),
                (".config", true),
                (".config/foot", true),
                (".config/foot/colors.ini", false),
                (".config/nvim", true),
                (".config/nvim/init.lua", false),
            ]),
        ));
        let p = plan(
            Path::new(HOME),
            &presets,
            &found(&[(".config", Found::Dir)]),
            &[],
        );
        assert_eq!(
            p.steps,
            [
                link(".bashrc", "/home/mj/.dotfiles/dev/.bashrc"),
                // Both have foot: a real folder with both files.
                Step::MakeDir(".config/foot".into()),
                link(
                    ".config/foot/colors.ini",
                    "/home/mj/.dotfiles/dev/.config/foot/colors.ini"
                ),
                link(
                    ".config/foot/foot.ini",
                    "/home/mj/.dotfiles/base/.config/foot/foot.ini"
                ),
                link(".config/nvim", "/home/mj/.dotfiles/dev/.config/nvim"),
            ]
        );
    }

    #[test]
    fn a_folded_link_is_unfolded_when_a_second_preset_comes() {
        let mut presets = base();
        presets.push((
            name("dev"),
            entries(&[
                (".config", true),
                (".config/foot", true),
                (".config/foot/colors.ini", false),
            ]),
        ));
        let p = plan(
            Path::new(HOME),
            &presets,
            &found(&[
                (".config", Found::Dir),
                (
                    ".config/foot",
                    Found::Link("/home/mj/.dotfiles/base/.config/foot".into()),
                ),
                // Seen through the old link – not real.
                (".config/foot/foot.ini", Found::File),
            ]),
            &[".config/foot".into()],
        );
        assert_eq!(
            p.steps[1..],
            [
                Step::Remove(".config/foot".into()),
                Step::MakeDir(".config/foot".into()),
                link(
                    ".config/foot/colors.ini",
                    "/home/mj/.dotfiles/dev/.config/foot/colors.ini"
                ),
                link(
                    ".config/foot/foot.ini",
                    "/home/mj/.dotfiles/base/.config/foot/foot.ini"
                ),
            ]
        );
    }

    #[test]
    fn links_no_longer_wanted_are_removed() {
        let p = plan(
            Path::new(HOME),
            &[],
            &found(&[
                (
                    ".bashrc",
                    Found::Link("/home/mj/.dotfiles/base/.bashrc".into()),
                ),
                // Replaced by the user meanwhile: not touched.
                (".profile", Found::File),
            ]),
            &[".bashrc".into(), ".profile".into()],
        );
        assert_eq!(p.steps, [Step::Remove(".bashrc".into())]);
        assert_eq!(p.links, Vec::<PathBuf>::new());
    }

    #[test]
    fn a_file_in_a_later_preset_hides_a_folder() {
        let mut presets = base();
        presets.push((
            name("dev"),
            entries(&[(".config", true), (".config/foot", false)]),
        ));
        let p = plan(
            Path::new(HOME),
            &presets,
            &found(&[(".config", Found::Dir)]),
            &[],
        );
        assert_eq!(
            p.steps[1..],
            [link(".config/foot", "/home/mj/.dotfiles/dev/.config/foot")]
        );
    }

    #[test]
    fn probe_and_apply_formats() {
        let out = "L\t.bashrc\t/home/mj/.dotfiles/base/.bashrc\nD\t.config\nF\t.profile\nX\tjunk\n";
        assert_eq!(
            parse_probe(out),
            found(&[
                (
                    ".bashrc",
                    Found::Link("/home/mj/.dotfiles/base/.bashrc".into())
                ),
                (".config", Found::Dir),
                (".profile", Found::File),
            ])
        );
        assert_eq!(
            apply_args(&[
                Step::Remove("a".into()),
                Step::MakeDir("b".into()),
                link("c", "/t"),
            ]),
            ["rm", "a", "mkdir", "b", "ln", "c", "/t"]
        );
        let links = [PathBuf::from(".bashrc"), ".config/foot".into()];
        assert_eq!(parse_links(&links_text(&links)), links);
        assert_eq!(
            probe_paths(&base(), &[".old".into()]),
            [
                PathBuf::from(".bashrc"),
                ".config".into(),
                ".config/foot".into(),
                ".config/foot/foot.ini".into(),
                ".old".into()
            ]
        );
    }

    #[test]
    fn scan_a_preset() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::create_dir_all(d.join(".config/foot")).unwrap();
        std::fs::create_dir_all(d.join(".git/objects")).unwrap();
        std::fs::write(d.join(".config/foot/foot.ini"), "[main]\n").unwrap();
        std::fs::write(d.join(".bashrc"), "").unwrap();
        std::os::unix::fs::symlink(d.join(".config"), d.join("link")).unwrap();
        assert_eq!(
            scan(d).unwrap(),
            entries(&[
                (".bashrc", false),
                (".config", true),
                (".config/foot", true),
                (".config/foot/foot.ini", false),
                ("link", false),
            ])
        );
    }
}
