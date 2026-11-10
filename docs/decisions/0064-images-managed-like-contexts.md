# 0064. Images are managed like contexts

- Status: Accepted (built-in images read-only: amended by ADR 0065)
- Date: 2026-11-10
- Source: discussion (user, 2026-11-10: "koje images should be managed similar
  to the koje contexts … configured once and visible in the koje settings
  app"); ADR 0040, 0063; plan G.39, G.40

## Context

ADR 0040 made images folders that contexts name; ADR 0063 gave each context a
one-step way to its own Containerfile from the context's page. An image
shared by several contexts then has no place of its own: it shows up only
through the contexts that use it, has no name for people, and cannot be
built, renamed or removed from koje Settings.

## Decision

- **An image is configured once, in its folder**, as a context is in its
  directory: `~/.config/koje/images/<name>/` holds the `Containerfile` (or
  `Dockerfile`), what it copies, and **`image.toml`**:

  ```toml
  label       = "Web development"   # shown instead of the name; optional
  description = "Node, VS Code"     # optional
  ```

  `image.toml` is optional (a folder without it is an image named by its
  folder) and refuses unknown keys. koje writes it when it creates an image.
- **koje's built-in images** (`fedora`) are listed too, read-only: their
  label and description come with koje, they cannot be edited or removed,
  and new images are made from them.
- **koje Settings lists images like contexts**: an "Images" section in the
  sidebar under the contexts, with + for a new one. An image's page shows
  its label and description (editable), its Containerfile (opened with the
  desktop's editor, ADR 0063), the contexts that use it, whether it is
  built, a Build button whose output is shown live, and Remove (refused
  while a context uses it).
- **A context picks its image from a dropdown** of the listed images. "New
  Image…" on the context's page asks for a name (default: the context's)
  and a label, creates the image `FROM` the context's current image and
  switches the context to it. This replaces ADR 0063's Create/Edit
  Containerfile on the context's page; `koje image edit <context>` stays as
  the CLI's shortcut and now writes `image.toml` too.
- **Registry images and folders at a path are not listed.** A registry image
  is used by making an image `FROM` it; a context that names one (or a
  path) directly still works and shows it in its dropdown as it is.
- kojed's API: `Images`, `NewImage`, `SetImage`, `RemoveImage`, `BuildImage`
  and the signal `ImagesChanged`; a build reports through
  `ImageBuildStarted`, `ImageBuildProgress`, `ImageBuildFinished` and
  `ImageBuildFailed` (an image may share a context's name, so the context
  operation signals are not reused). `SetConfig` takes `image`. `ImageFile`
  (ADR 0063) is removed.
- CLI: `koje image list`, `koje image new <name> [--from] [--label]`,
  `koje image rm <name>`, besides `build` and `edit`.

## Consequences

+ A shared image is configured and built in one place and visible as such.
+ Contexts and images look and work alike in koje Settings.
- One more file per image; images made before have none until edited
  (their folder name is shown).
- Renaming an image is not offered: contexts name it by its folder.
- Supersedes ADR 0063's settings-app part.
