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

# Build docs for all crates
doc:
    cargo doc --workspace --no-deps

# Install koje and kojed into ~/.cargo/bin and koje's assets (run on the HOST)
install: install-assets
    cargo install --locked --path crates/koje
    cargo install --locked --path crates/kojed

# Install koje's assets for the current user (icon font, ADR 0020)
install-assets:
    #!/usr/bin/env bash
    set -euo pipefail
    fonts="${XDG_DATA_HOME:-$HOME/.local/share}/fonts/koje"
    mkdir -p "$fonts"
    cp assets/fonts/lucide/lucide.ttf assets/fonts/lucide/LICENSE "$fonts/"
    if command -v fc-cache >/dev/null; then fc-cache -f "$fonts"; fi
    echo "installed icon font to $fonts"

# Render the concept document (needs the `typst` CLI; Tinymist in VS Code can preview too)
concept:
    typst compile docs/concept/koje-konzept.typ docs/concept/koje-konzept.pdf
