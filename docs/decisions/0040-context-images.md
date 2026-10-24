# 0040. Context images: image folders, registry images, a koje layer and kept versions

- Status: Proposed
- Date: 2026-10-24
- Source: discussion (how to install VS Code and other programs in contexts), ADR 0023

## Context

ADR 0023 gives every context the locally built `koje/base` and lets a context
name another image (`image = …`). How users get programs such as VS Code into
contexts was left open. Installing into a running context is lost on a recreate
(ADR 0033); installing into the home volume duplicates every program per
context and loses shared layers (M2.6). Users want their own images, different
ones per context, other distributions than Fedora, and images from registries
(official koje images may be published later).

Two things stop that today. `koje/base` bakes the host user (name, uid, home,
sub-ids) into the image, so no published image can run as a context. And parts
of it are Fedora-specific (`rpm --restore shadow-utils`, the GBM directory
`/usr/lib64/gbm` in `koje_core::spec`).

Rebuilt images have a second problem, well known from Docker: nothing tells
that an image is outdated except rebuilding it, and a rebuild can break what
worked.

## Decision

### Two layers: the image and koje's user layer

- An **image** holds a distribution and its programs. It knows nothing about the
  host user, so it can be shared and published.
- koje always adds a thin **user layer** on top before a context uses an
  image. Its script is compiled into koje and written in POSIX `sh`, so it is
  the same for every distribution. It:
  - creates the host user with the same name, uid and home, and writes their
    sub-ids;
  - gives `newuidmap`/`newgidmap` their file capabilities with `setcap`;
  - writes the inner Podman's `containers.conf` and `storage.conf` (ADR 0014);
  - removes the image's portal D-Bus services (ADR 0019).
  It writes paths that depend on the distribution as labels that koje reads
  when it creates the container. The first is the GBM backend directory
  (`io.koje.gbm-dir`), which replaces the fixed `IMAGE_GBM_DIR`.
- **Requirements** for an image are documented: `sh`, `useradd` and `setcap`,
  and for each access switch what it needs (e.g. `nested` needs `podman`,
  `fuse-overlayfs` and `newuidmap`; the session bus needs `dbus-daemon`). After
  the user layer is built, koje checks for them and warns about what is
  missing. It does not refuse: a context without `nested` does not need Podman.
- Anything that is a matter of taste stays in the image, e.g. the Firefox
  `mimeapps.list` or Starship.

### Where images come from: `image = …` in `context.toml`

Short values with one default, so that 99 % of users never write more than a
name:

| Value | Meaning |
|---|---|
| `fedora` (a plain name) | the **image folder** `~/.config/koje/images/fedora/`, else the koje image of that name built into koje |
| `~/…` or `/…` | an image folder anywhere, e.g. in a project repository |
| anything else (`ghcr.io/…/fedora:44`, `docker.io/library/debian:13`) | a **registry image**, pulled by Podman |

- An **image folder** holds a `Containerfile` (or `Dockerfile`) and is its
  build context, so `COPY` works. It is laid out the same way as dotfiles
  presets (ADR 0038). An image that only downloads is a one-line
  `FROM ghcr.io/…`.
- koje ships **`fedora`**, today's `koje/base` without the user part. It is the
  default for new contexts. A user folder with the same name takes the
  built-in image's place; `koje doctor` says so. The old value
  `koje/base:latest` is read as `fedora`, with a hint.
- Images build on each other with `FROM localhost/koje/<name>`. koje reads the
  `FROM` lines and builds parents first.
- The settings app (ADR 0036) writes the same values. It can offer a picker
  (built-in images, folders, path, registry) without needing a different file
  format.

### Versions: every build is kept and can be switched back to

- Images are tagged `localhost/koje/<name>:latest`; `FROM` lines use that tag.
- Images with the user layer are tagged `localhost/koje/user/<name>:<version>`.
  The version is the build date plus a counter (`2026-10-24.1`). One version
  per image is **current**; a new build becomes current.
- **A build never changes a context.** A context changes only when its
  container is recreated (`koje recreate`, or `koje up` asking, ADR 0033), and
  it then gets the current version. A new build can be tried in one context
  before the others follow.
- **Rolling back:** `koje image rollback <image> [<version>]` makes the
  previous version (or the given one) current again. Contexts on the broken
  version then show as outdated and come back with a recreate. Their home is
  untouched.
- koje never removes a version on its own; reclaiming space is the user's
  choice (see below).

### Outdated: reported, never applied silently

- **A context on an older version than current:** koje compares the image ID of
  the container with that of the current version. `koje list`, `koje up` and
  `koje doctor` show it and point to `koje recreate`. `koje recreate
  --outdated` recreates all of them (listed first, one question).
- **An image built on a parent that was rebuilt since:** koje records the
  parent's image ID as the label `io.koje.parent` at build time and compares
  it.
- **An image older than its sources** (distribution updates, a new VS Code)
  cannot be known without rebuilding. koje shows each image's age and does not
  guess. `koje image update` rebuilds without cache and pulls: a new version,
  which can be rolled back.

### Recreate: the old container is kept until the user reclaims the space

- A recreate does not remove the old container. koje stops it and renames it
  to `koje-<context>-<date>`. It keeps its label `io.koje.context` (Podman
  cannot change labels), so koje finds the old containers of a context by
  that label and by any name other than `koje-<context>`. koje never starts
  an old container: it still points at the context's shared home.
- Before the recreate question, koje lists what changed outside `$HOME` in the
  container's file system (`podman diff`). A pure function sorts the changes:
  - **noise**, which is not shown: `/tmp`, `/var/tmp`, `/run`, `/var/cache`,
    `/var/log`;
  - **installed programs** under `/usr`, which are counted: "412 files under
    /usr changed (installed programs) – put them into an image";
  - **everything else** (`/etc`, `/opt`, `/srv`, `/root`, `/var/lib`, …),
    which is listed.
- **Recovering:** `koje recover <context>` lists the changes the old containers
  have outside the home and copies the chosen paths into the context's home,
  below `~/recovered/<date>/`.
- `koje rm` removes a context's old containers before its home volume (Podman
  does not remove a volume a container uses).

### Disk space: shown, and reclaimed in one step

- koje tracks what it keeps beyond what contexts use: old containers (their
  own changes, `podman ps --size`) and image versions that are neither current
  nor used by a container (their own layers, `podman system df -v`).
- `koje prune` shows these with their sizes, then asks one question. It
  removes old containers first, because they keep their image versions in
  use. Then it removes the image versions no container uses any more, except
  the current version and the one before it (one rollback step stays
  possible). An old container with changes outside the home is listed with
  them, so nothing goes unseen. `--all` also removes the version before the
  current one; `--dry-run` only shows.
- The settings app (ADR 0036) shows the same: the reclaimable space in total
  and per context and image, the changes outside the home of each old
  container, and one button that reclaims everything, with single items
  removable as well.

### CLI

| Command | Purpose |
|---|---|
| `koje image list` | images, where each comes from, current version, age, kept versions, contexts using each version, outdated marks |
| `koje image new <name> [--from <image>]` | create `~/.config/koje/images/<name>/Containerfile` with a `FROM` line and comments |
| `koje image build [<image>…] [--no-cache] [--pull] [--dependents]` | build images (default: those used by contexts); `--dependents` also rebuilds images built on them |
| `koje image update [<image>…]` | `build --no-cache --pull --dependents` |
| `koje image rollback <image> [<version>]` | make an older version current again |
| `koje recreate --outdated` | recreate every context on an older version |
| `koje recover <context>` | copy files from the old containers' changes outside the home into the home |
| `koje prune [--all] [--dry-run]` | show and reclaim the space of old containers and unused image versions |

## Consequences

+ Programs such as VS Code go into an image folder (a few lines of
  Containerfile), are shared between contexts with their layers, and survive
  recreates.
+ Any distribution and any registry image can be a context's image, including
  future official koje images, because the host user is always added locally.
+ A broken rebuild costs one `koje image rollback` and a recreate.
- The user layer must stay portable `sh`. Each new distribution-dependent path
  becomes a label and is tested against more than Fedora.
- Kept versions cost disk space until pruned (shared layers keep it small).
- koje still cannot tell that an image's sources changed; it shows the age and
  leaves rebuilding to the user.
- Supersedes parts of ADR 0023: the name `koje/base`, the user being part of
  the image, and `koje image build` building only one image.
+ A recreate loses nothing until the user reclaims the space: files outside
  the home can be recovered from the old container, and koje names programs
  installed at runtime as something that belongs in an image.
+ Disk space koje keeps is visible in one place and reclaimed in one step.
- Old containers and image versions use disk space until the user prunes.
- Every place that lists containers by `io.koje.context` must tell old
  containers from current ones.
