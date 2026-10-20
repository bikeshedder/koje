//! What kojed reads about a window's process from the host's `/proc`
//! (ADR 0031): command line, working directory, direct children. Paths are as
//! the process sees them (inside its context).

use std::path::Path;

use koje_core::capture::Proc;

fn read_one(proc_root: &Path, pid: u32) -> Option<Proc> {
    let dir = proc_root.join(pid.to_string());
    let raw = std::fs::read(dir.join("cmdline")).ok()?;
    let argv = raw
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    let cwd = std::fs::read_link(dir.join("cwd"))
        .ok()
        .map(|p| p.display().to_string());
    Some(Proc {
        argv,
        cwd,
        children: Vec::new(),
    })
}

/// Children of `pid` over all its threads (`/proc/<pid>/task/*/children`).
fn children(proc_root: &Path, pid: u32) -> Vec<u32> {
    let Ok(tasks) = std::fs::read_dir(proc_root.join(pid.to_string()).join("task")) else {
        return Vec::new();
    };
    let mut pids: Vec<u32> = tasks
        .filter_map(std::result::Result::ok)
        .filter_map(|t| std::fs::read_to_string(t.path().join("children")).ok())
        .flat_map(|text| {
            text.split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect::<Vec<u32>>()
        })
        .collect();
    pids.sort_unstable();
    pids.dedup();
    pids
}

/// The process with its direct children; `None` if it is gone.
pub fn observe(proc_root: &Path, pid: u32) -> Option<Proc> {
    let mut p = read_one(proc_root, pid)?;
    p.children = children(proc_root, pid)
        .into_iter()
        .filter_map(|c| read_one(proc_root, c))
        .collect();
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_fake_proc() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mk = |pid: u32, cmdline: &[u8], cwd: &str| {
            let d = root.join(pid.to_string());
            std::fs::create_dir_all(d.join("task").join(pid.to_string())).unwrap();
            std::fs::write(d.join("cmdline"), cmdline).unwrap();
            std::os::unix::fs::symlink(cwd, d.join("cwd")).unwrap();
        };
        mk(10, b"foot\0--server\0", "/home/mj");
        mk(11, b"-bash\0", "/home/mj/src/kp");
        std::fs::write(root.join("10/task/10/children"), "11 99 ").unwrap();
        let p = observe(root, 10).unwrap();
        assert_eq!(p.argv, ["foot", "--server"]);
        assert_eq!(p.cwd.as_deref(), Some("/home/mj"));
        assert_eq!(p.children.len(), 1, "a vanished child (99) is skipped");
        assert_eq!(p.children[0].cwd.as_deref(), Some("/home/mj/src/kp"));
        assert_eq!(observe(root, 12), None);
    }

    #[test]
    fn reads_the_real_proc() {
        let me = observe(Path::new("/proc"), std::process::id()).unwrap();
        assert_ne!(me.argv, Vec::<String>::new());
        assert!(me.cwd.is_some());
    }
}
