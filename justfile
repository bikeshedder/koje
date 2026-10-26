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

# Build the koje package into target/debian/ (ADR 0045); on the host:
# sudo apt install ./target/debian/koje_<version>_amd64.deb && koje setup
deb:
    scripts/build-plugin.sh
    cargo build --release --locked -p koje -p kojed -p koje-shell
    cargo build --release --locked -p koje-forward --target x86_64-unknown-linux-musl
    mkdir -p target/deb-staging
    cp target/x86_64-unknown-linux-musl/release/koje-forward target/deb-staging/
    cargo deb -p koje --no-build --deb-version "$(scripts/deb-version.sh)"

# On the HOST: install the newest package from target/debian/ and run koje setup
install *ARGS:
    scripts/install-deb.sh {{ARGS}}

# Render the concept document (needs the `typst` CLI; Tinymist in VS Code can preview too)
concept:
    typst compile docs/concept/koje-konzept.typ docs/concept/koje-konzept.pdf
