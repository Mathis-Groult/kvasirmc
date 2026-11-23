#!/bin/bash
# Rebuilds probe.wasm (wasm32-wasip2) next to the golden test that loads it.
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --target wasm32-wasip2 --offline
cp target/wasm32-wasip2/release/probe_plugin.wasm ../probe.wasm
