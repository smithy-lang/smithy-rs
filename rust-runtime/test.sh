#!/bin/bash
# Currently avoiding creating a workspace of the runtime packages
set -e
for crate in ./*/
do
  echo "Testing $crate"
  (cd "$crate" && cargo fmt -- --check)
  (cd "$crate" && cargo test)
done