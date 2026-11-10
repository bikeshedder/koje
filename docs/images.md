# Images

A context's programs come from its **image** (ADR 0040). koje ships one,
`fedora`; your own are **image folders**: a `Containerfile` (or `Dockerfile`)
and what it copies, in `~/.config/koje/images/<name>/`. Programs installed in
an image survive `koje recreate` and are shared, layer by layer, by every
context that uses it.

## Choosing a context's image

`image = …` in `context.toml`:

| Value | Meaning |
|---|---|
| `fedora` (a plain name) | the image folder `~/.config/koje/images/fedora/`, else koje's built-in image |
| `~/src/app/image` or `/…` | an image folder anywhere, e.g. in a repository |
| `docker.io/library/debian:13` | a registry image, pulled by Podman |

koje adds your user on top of every image (its *user layer*), so any of these
can run a context.

## A context's own image

`koje image edit <context>` (or *Edit Containerfile* in koje Settings) gives
the context an image folder named after it, building on the image it has,
and opens the `Containerfile` in your desktop's editor (ADR 0063).

`koje image new <name> [--from <image>]` creates an image folder to share;
use it with `image = "<name>"`.

## When images are built

When a context's container is created – its first `koje up`, or
`koje recreate` – koje builds what is missing or changed: the images it
builds on first (`FROM localhost/koje/<name>`), then the image, then your
user on top. A build never changes a container that exists: `koje up` tells
you when a context's image changed and asks to recreate it.

`koje image build [<image>…]` builds now (default: the images contexts use);
`--pull` fetches newer base images, `--no-cache` rebuilds every layer.

## Example: VS Code

`koje image new code`, then in `~/.config/koje/images/code/Containerfile`:

```Dockerfile
FROM localhost/koje/fedora:latest
RUN rpm --import https://packages.microsoft.com/keys/microsoft.asc \
    && printf '[code]\nname=Visual Studio Code\nbaseurl=https://packages.microsoft.com/yumrepos/vscode\nenabled=1\ngpgcheck=1\ngpgkey=https://packages.microsoft.com/keys/microsoft.asc\n' \
        > /etc/yum.repos.d/vscode.repo \
    && dnf -y install code \
    && dnf clean all
```

and `image = "code"` in the contexts that want it, then `koje recreate <context>`.

## What an image needs

koje checks every image when it adds your user and refuses one without:

- `useradd` (koje adds your user with it),
- `dbus-daemon` (every context runs its session bus).

It warns when a context's access needs more:

- `nested` (containers inside): `podman`, `fuse-overlayfs`, `newuidmap`,
  `newgidmap`, `setcap`.

On Debian or Ubuntu these are the packages `passwd`, `dbus`, `podman`,
`fuse-overlayfs`, `uidmap` and `libcap2-bin`; programs with windows need
their usual libraries (Mesa for graphics).
