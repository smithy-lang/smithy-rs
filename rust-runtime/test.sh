#!/bin/bash

set -e
for crate in "$(dirname "$0")"/*/
do
  if [ -d "$crate" ] && [ -f "$crate/Cargo.toml" ]; then
    echo "Testing $crate"
    (cd "$crate" && cargo fmt)
    (cd "$crate" && cargo fmt -- --check)
    (cd "$crate" && cargo clippy -- -D warnings)
    (cd "$crate" && cargo test)
  fi
done
