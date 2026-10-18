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

# Install koje on this machine (run on the HOST; see install.sh --help)
install *ARGS:
    ./install.sh {{ARGS}}

# Render the concept document (needs the `typst` CLI; Tinymist in VS Code can preview too)
concept:
    typst compile docs/concept/koje-konzept.typ docs/concept/koje-konzept.pdf
