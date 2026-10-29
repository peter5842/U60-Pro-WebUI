#!/bin/bash
set -euo pipefail
ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
cd "$ROOT_DIR"
echo 'Building zte-screen…'
cargo build --locked --release --target aarch64-unknown-linux-musl -p zte-screen
exec python3 "$ROOT_DIR/scripts/deploy-components.py" --screen "$ROOT_DIR/target/aarch64-unknown-linux-musl/release/zte-screen" "$@"
