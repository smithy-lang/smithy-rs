# What's this?

This is a cli app that let's you to benchmark the compile time.
It is part of RFC30.

# How to use

```bash
# set `IS_DRY_RUN` to FALSE if you want to turn off the dry run. 
# export IS_DRY_RUN=FALSE

# create necessary resources on aws
cargo run --release --bin initial-setup

# submit job to batch
cargo run --release --bin submit-job

# delete all resources on aws
cargo run --release --bin delete-all
```
