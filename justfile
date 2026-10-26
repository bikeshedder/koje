# Task runner for koje. Run `just` to list recipes.

set shell := ["bash", "-euo", "pipefail", "-c"]

# List recipes
default:
    @just --list

# Everything CI checks: formatting, lints, tests
check: fmt-check lint test

# Format all code
fmt:
    cargo fmt --all

# Fail if code is not formatted
fmt-check:
    cargo fmt --all -- --check

# Clippy with warnings as errors
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Run all tests (pass extra args, e.g. `just test layout`)
test *ARGS:
    cargo test --workspace {{ARGS}}

# Build all crates
build:
    cargo build --workspace

# Run the CLI, e.g. `just koje list`
koje *ARGS:
    cargo run --quiet -p koje -- {{ARGS}}

# Run the daemon in the foreground with debug logging
kojed:
    RUST_LOG=${RUST_LOG:-debug} cargo run -p kojed

# Run the shell (bar) in the foreground with debug logging; needs a Wayland session
shell:
    RUST_LOG=${RUST_LOG:-koje_shell=debug} cargo run -p koje-shell

# Build docs for all crates
doc:
    cargo doc --workspace --no-deps

# Build the koje package into target/debian/ in an Ubuntu 26.04 container
# (ADR 0045; Podman, on the host or here); on the host:
# sudo apt install ./target/debian/koje_<version>_amd64.deb && koje setup
deb:
    scripts/build-deb.sh

# On the HOST: install the newest package from target/debian/ and run koje setup
install *ARGS:
    scripts/install-deb.sh {{ARGS}}

# Render the concept document (needs the `typst` CLI; Tinymist in VS Code can preview too)
concept:
    typst compile docs/concept/koje-konzept.typ docs/concept/koje-konzept.pdf
