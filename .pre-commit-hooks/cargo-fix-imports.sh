#!/bin/bash
cd rust-runtime &&
cargo fix --allow-dirty &&
cargo fmt && 
cd --