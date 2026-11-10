# Images

A context's programs come from its **image** (ADR 0040). Programs installed
in an image survive `koje recreate` and are shared, layer by layer, by every
context that uses it. koje adds your user on top of every image (its *user
layer*): contexts run as you, with your name, uid and home, and with `sudo`
without a password unless the access switch `sudo` is off (ADR 0066).

Images are kept like contexts (ADR 0064): koje Settings lists them under the
contexts, each with its page.

## Names

An image has one name everywhere (ADR 0067): `localhost/koje/<name>:<version>`.
It is what `context.toml` says, what a `FROM` line names, and what Podman
calls it. koje never uses `:latest`.

koje's own images carry their release:

| Image | |
|---|---|
| `localhost/koje/fedora:45` | Fedora 45 |
| `localhost/koje/ubuntu:26.04` | Ubuntu 26.04 LTS (Firefox from Mozilla's APT repository) |
| `localhost/koje/debian:13` | Debian 13 (Firefox ESR) |

All three have Firefox, foot, Nautilus, Git, Podman for dev containers,
sudo, Mesa, the sound and printing clients and fonts.

Your own images are folders in `~/.config/koje/images/<name>/`: a
`Containerfile` (or `Dockerfile`), what it copies, and `image.toml`:

```toml
label       = "Web development"   # shown instead of the name
description = "Node, VS Code"
version     = "1"                 # its tag: localhost/koje/web:1
```

## A context's image

`image = …` in `context.toml`:

| Value | Meaning |
|---|---|
| `localhost/koje/<name>:<version>` | the image folder `<name>`, else koje's own image of that name |
| `~/src/app/image` or `/…` | an image folder anywhere, e.g. in a repository |
| `docker.io/library/debian:13` | a registry image, pulled by Podman |

A new context's image is chosen when it is made: koje Settings' *New
Context*, or `koje new <name> --image localhost/koje/ubuntu:26.04` (asked on a
terminal without `--image`). Older values (`fedora`, `koje/base:latest`) are
still read, with a hint to write the full name.

## Making and changing images

- **koje Settings**: a context's page has its image as a dropdown and *New
  Image…*, which makes an image from the one it has and switches the context
  to it. An image's page edits its label, description, name and version,
  opens its `Containerfile` in your desktop's editor or its folder in the
  file manager, builds it with the output shown live, and lists the contexts
  using it.
- **CLI**: `koje image new <name> --from <image> [--label <label>]`,
  `koje image list`, `koje image rename <name> <new-name>`,
  `koje image rm <name>`, and `koje image edit <context>`, which gives a
  context an image of its own (named after it) and opens its `Containerfile`.

Renaming an image, or giving it a new version, changes every context that
uses it and every image folder whose `FROM` names it in the same step.

koje's own images are updated with koje. To change one for every context,
*Customize…* it on its page (or `koje image customize fedora`): koje copies
its files into `~/.config/koje/images/fedora/`, which then takes its place
and is edited like any image. When a later koje ships a newer version, the
image is marked "Newer version from koje": *Keep Mine*, or *Reset to koje's
Version* (`koje image reset fedora`), which deletes your copy (ADR 0065).

## When images are built

When a context's container is created – its first `koje up`, or
`koje recreate` – koje builds what is missing or changed: the images it
builds on first, then the image, then your user on top. A build never
changes a container that exists: `koje up` tells you when a context's image
changed and asks to recreate it.

`koje image build [<image>…]` builds now (default: the images contexts use);
`--pull` fetches newer base images, `--no-cache` rebuilds every layer.

## Example: VS Code on Fedora

`koje image new code --from localhost/koje/fedora:45`, then in
`~/.config/koje/images/code/Containerfile`:

```Dockerfile
FROM localhost/koje/fedora:45
RUN rpm --import https://packages.microsoft.com/keys/microsoft.asc \
    && printf '[code]\nname=Visual Studio Code\nbaseurl=https://packages.microsoft.com/yumrepos/vscode\nenabled=1\ngpgcheck=1\ngpgkey=https://packages.microsoft.com/keys/microsoft.asc\n' \
        > /etc/yum.repos.d/vscode.repo \
    && dnf -y install code \
    && dnf clean all
```

and `image = "localhost/koje/code:1"` in the contexts that want it, then
`koje recreate <context>`.

## What an image needs

koje checks every image when it adds your user and refuses one without:

- `useradd` (koje adds your user with it),
- `dbus-daemon` (every context runs its session bus).

It warns when a context's access needs more:

- `nested` (containers inside): `podman`, `fuse-overlayfs`, `newuidmap`,
  `newgidmap`, `setcap`;
- `sudo`: `sudo`.

On Debian or Ubuntu these are the packages `passwd`, `dbus-daemon`,
`podman`, `fuse-overlayfs`, `uidmap`, `libcap2-bin` and `sudo`; programs
with windows need their usual libraries (Mesa for graphics).
