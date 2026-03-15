#!/usr/bin/env bash
set -euo pipefail

cargo test
cargo clippy --all-targets -- -D warnings
