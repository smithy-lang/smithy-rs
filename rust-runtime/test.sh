#!/bin/bash
# Currently avoiding creating a workspace of the runtime packages
set -e
for crate in "$(dirname "$0")"/*/
do
  if [ -d "$crate" ] && [ -f "$crate/Cargo.toml" ]; then
    echo "Testing $crate"
    (cd "$crate" && cargo fmt -- --check)
    (cd "$crate" && cargo clippy -- -D warnings)
    (cd "$crate" && cargo test)
  fi
done
