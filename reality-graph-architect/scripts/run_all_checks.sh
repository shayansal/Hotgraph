#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"

cd "${repo_root}"

cargo_cmd="${CARGO:-cargo}"
if ! command -v "${cargo_cmd}" >/dev/null 2>&1; then
  if command -v cargo.exe >/dev/null 2>&1; then
    cargo_cmd="cargo.exe"
  else
    echo "cargo not found. Install Rust or set CARGO=/path/to/cargo." >&2
    exit 127
  fi
fi

"${cargo_cmd}" fmt --all
"${cargo_cmd}" clippy --all-targets --all-features -- -D warnings
"${cargo_cmd}" test --all
"${cargo_cmd}" test --all --release
