# Development container for koje.
#
# Rust toolchain from the official dev container image (Debian 13 "trixie"),
# plus just and Podman. Podman runs the container integration tests inside the
# dev container (nested Podman, see docs/development.md), set up like nested
# Podman in contexts (ADR 0014).
FROM mcr.microsoft.com/devcontainers/rust:1-trixie

ARG USERNAME=vscode

RUN apt-get update \
    && export DEBIAN_FRONTEND=noninteractive \
    && apt-get install -y --no-install-recommends \
        just \
        podman \
        catatonit \
        fuse-overlayfs \
        uidmap \
        passt \
        jq \
        ripgrep \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# The base image pins a numbered toolchain (e.g. 1.99.0), not the `stable`
# channel that rust-toolchain.toml asks for. Without this step rustup
# auto-installs it on first use, and rust-analyzer and postCreateCommand race
# on that install ("cargo ... is not applicable to the toolchain").
# Run as the user so the new toolchain is owned by them, like rustup updates.
COPY rust-toolchain.toml /tmp/toolchain/
USER ${USERNAME}
RUN cd /tmp/toolchain && rustup toolchain install && rustc --version
USER root

# Nested Podman (ADR 0014). newuidmap/newgidmap need file capabilities: setuid
# root cannot write uid_map in the dev container's user namespace. Sub-ids are
# every id the dev container has except the user's own (Debian's default
# 100000+ is not mapped). The dev container's cgroups are read-only, /proc is
# partly masked and the kernel's sysctls are not ours: no cgroups, the outer
# /proc, no default sysctls.
RUN chmod u-s /usr/bin/newuidmap /usr/bin/newgidmap \
    && setcap cap_setuid=ep /usr/bin/newuidmap \
    && setcap cap_setgid=ep /usr/bin/newgidmap \
    && printf '%s:1:999\n%s:1001:64535\n' ${USERNAME} ${USERNAME} > /etc/subuid \
    && cp /etc/subuid /etc/subgid \
    && mkdir -p /etc/containers/containers.conf.d \
    && printf '%s\n' \
        '[containers]' \
        'cgroups = "disabled"' \
        'default_sysctls = []' \
        'volumes = ["/proc:/proc"]' \
        '[engine]' \
        'cgroup_manager = "cgroupfs"' \
        'events_logger = "file"' \
        > /etc/containers/containers.conf.d/koje-dev.conf

# Mount points for named volumes must exist and belong to the user, otherwise
# Podman creates them root-owned on first use.
RUN mkdir -p /home/${USERNAME}/.claude /home/${USERNAME}/.local/share/containers \
    && chown -R ${USERNAME}:${USERNAME} /home/${USERNAME}/.claude /home/${USERNAME}/.local
