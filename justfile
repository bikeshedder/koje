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

# Lint koje's GNOME Shell extension (ADR 0059; npm fetches ESLint once)
lint-extension:
    cd gnome && npm ci --no-audit --no-fund --silent && npx eslint extension

# Build koje's UI (ADR 0060): TypeScript and Blueprint in gnome/src to gnome/build
ui:
    cd gnome && npm ci --no-audit --no-fund --silent && npm run --silent build

# Check koje's UI: generated D-Bus types current, TypeScript, tests, ESLint
check-ui:
    cd gnome && npm ci --no-audit --no-fund --silent && npm run --silent check

# Run koje Settings from gnome/build against the session's kojed (`just ui` first)
settings *ARGS:
    gjs -m gnome/build/settings/main.js {{ARGS}}

# The GNOME adapter against the extension running in the GNOME lab
# (docs/spikes/setup/gnome-lab: lab.sh start first)
test-extension:
    DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/koje-gnome-lab/run/bus cargo test -p koje-gnome --test extension -- --ignored

# On the HOST, in a GNOME session: a nested GNOME Shell with the installed
# extension, its own kojed and a test context (scripts/nested-gnome.sh)
nested-gnome *ARGS:
    scripts/nested-gnome.sh {{ARGS}}

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
