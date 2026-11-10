# 0067. Images are named as Podman names them, with a version, and chosen per context

- Status: Accepted
- Date: 2026-11-10
- Source: discussion (user, 2026-11-10: "`image = "fedora"` seems a bit
  misleading … write `localhost/koje/fedora` … so it can be used when
  writing Containerfiles"; "never use `:latest` for the base image; the
  built-in images should be Fedora 45, Ubuntu 26.04 LTS and Debian 13";
  "the user chooses" the image of a new context; "rename images and open
  the directory containing the Containerfile"); ADR 0040, 0064, 0065;
  plan G.43–G.45

## Context

ADR 0040 let `image = …` name an image by a short name (`fedora`), and
koje tagged its images `localhost/koje/<name>:latest`. A Containerfile that
builds on an image needs Podman's name (`FROM localhost/koje/fedora:latest`),
so users met two names for one image, and `:latest` hid which release a
context runs. koje shipped one image, Fedora 44.

## Decision

- **One name everywhere**: an image is `localhost/koje/<name>:<version>`,
  in `context.toml` (`image = "localhost/koje/fedora:45"`), in `FROM` lines,
  in Podman and in koje Settings. koje never writes or tags `:latest`.
  koje's user layer is `localhost/koje/user/<name>:<version>`.
- **koje's own images carry their release**: `fedora:45`, `ubuntu:26.04`
  and `debian:13`, with the same programs (browser, terminal, file
  manager, tools, Podman, sudo); Ubuntu's Firefox comes from Mozilla's
  APT repository (the snap does not run in a container), Debian's is
  Firefox ESR. A later koje may add a release next to them (`fedora:46`);
  a context moves when its user chooses.
- **Your own images** have a `version` in `image.toml` (`"1"` for a new
  one). Changing it, or renaming an image, updates every context that
  uses it and every image folder whose `FROM` names it, in the same step.
  A customized built-in image (ADR 0065) keeps koje's version and name.
- **References are checked**: a reference to a koje image that does not
  exist in that version is an error naming the versions there are.
  Older values are still read, with a hint to write the new one:
  `koje/base:latest` and `fedora` as `localhost/koje/fedora:45`, a short
  image folder name as that folder's image.
- **The user chooses a new context's image**: koje Settings' *New Context*
  asks for it; `koje new <name> --image <image>` takes it, and asks on a
  terminal without it. There is no default image.
- **Renaming** (koje Settings: the image's name; CLI: `koje image rename`)
  moves the folder; built-in images are not renamed. An image's page has
  *Open Folder*, which shows the folder with its Containerfile in the
  file manager.

## Consequences

+ What `context.toml` says can be pasted after `FROM`, and the release a
  context runs is visible.
+ Contexts can run Fedora, Ubuntu or Debian side by side.
- Contexts made before are asked to recreate once (their image is now
  `fedora:45`, not Fedora 44).
- Three images to keep working; each is checked with the probe of ADR
  0040 when built.
- Amends ADR 0040 (short names, `:latest`), 0064 and 0065.
