# 0065. koje's own images can be customized

- Status: Accepted
- Date: 2026-11-10
- Source: discussion (user, 2026-11-10: "the koje base image should be managed
  by the same tooling"); ADR 0040, 0064; plan G.41

## Context

ADR 0064 lists koje's built-in images (`fedora`) with the user's image
folders but keeps them read-only: their files ship inside koje, and a koje
update may change them. Changing the base all contexts share meant a new
image `FROM` it and switching every context over.

## Decision

- **Customize on demand.** A built-in image stays koje's until the user
  customizes it (koje Settings: *Customize…* on its page; CLI:
  `koje image customize <image>`). koje then copies its files into
  `~/.config/koje/images/<name>/`, which takes the built-in image's place
  for every context (ADR 0040) and is an ordinary image from then on:
  Containerfile edited in the desktop's editor, label, description, Build.
- `image.toml` of a customized image records the version it was copied
  from: `from_koje = "<hash of koje's files>"`.
- **koje updates.** Untouched built-in images follow koje's updates as
  before (rebuilt when their files change, ADR 0040). A customized one is
  never changed by koje; when koje ships files with another hash than
  `from_koje`, the image is marked (sidebar, page, `koje image list`) and
  offers:
  - *Reset to koje's Version* (`koje image reset <image>`): the folder is
    removed and koje's image applies again;
  - *Keep Mine*: `from_koje` is set to the new hash; the mark goes away.
- A customized image cannot be removed, only reset: removing it is what
  resetting does, and it stays allowed while contexts use it, since koje's
  image takes its place.

## Consequences

+ The base every context shares is edited where it lives, with the same
  tools as any image.
+ Untouched installs keep getting koje's image updates.
- After customizing, merging koje's later changes is the user's: the mark
  says that there is a newer version; the user compares or resets.
- Amends ADR 0064 (built-in images were read-only).
