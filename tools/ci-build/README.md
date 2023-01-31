ci-build
========

This directory includes everything to build the build/release/CI Docker image.
- `Dockerfile`: Dockerfile used to create the base build image. Needs to be in `tools/ci-build` so that it
  can copy all the tools source code into the image at build time.
- `add-local-user.dockerfile`: Creates a user in the build image with the host's user ID
- `build.docker-compose.yml`: Docker Compose file for using the build image
- `ci-action`: Script for running CI actions inside the Docker build image
- `ci-create-workspace`: Used by `ci-action`, but can be run manually to create a one-off workspace for debugging
- `sanity-test`: Script that sanity tests the Docker build image
- Other directories include various tools written in Rust for build/release/CI

There are three spaces you need to conceptualize for testing this locally:
- **Origin:** The original `smithy-rs` where you're iterating on CI scripts.
- **Starting space:** Directory with `smithy-rs` to run CI checks against. You have to create this. Conceptually,
  this is equivalent to the GitHub Actions working directory.
- **Action space:** Temporary directory maintained by `ci-action` where the CI checks actually run.

To create the starting space, do the following:

```bash
cd /path/to/my/starting-space
git clone https://github.com/awslabs/smithy-rs.git
# Optionally check out the revision you want to work with in the checked out smithy-rs.
# Just make sure you are in /path/to/my/starting-space (or whatever you called it) after.
```

Then you can test CI against that starting space by running:
```bash
$ORIGIN_PATH/tools/ci-build/ci-action <action> [args...]
```

The action names are the names of the scripts in `tools/ci-scripts/`, and `[args...]` get forwarded to those scripts.

__Note:__ `ci-action` does not rebuild the build image, so if you modified a script,
you need to run `./acquire-build-image` from the origin `.github/scripts` path.
