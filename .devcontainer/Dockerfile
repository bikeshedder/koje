# Development container for koje.
#
# Rust toolchain from the official dev container image (Debian 13 "trixie"),
# plus just and Podman. Podman is here so later milestones can run container
# integration tests inside the dev container (nested Podman, see
# docs/development.md); it is not needed for M0–M2.
FROM mcr.microsoft.com/devcontainers/rust:1-trixie

ARG USERNAME=vscode

RUN apt-get update \
    && export DEBIAN_FRONTEND=noninteractive \
    && apt-get install -y --no-install-recommends \
        just \
        podman \
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

# Mount points for named volumes must exist and belong to the user, otherwise
# Podman creates them root-owned on first use.
RUN mkdir -p /home/${USERNAME}/.claude \
    && chown -R ${USERNAME}:${USERNAME} /home/${USERNAME}/.claude
