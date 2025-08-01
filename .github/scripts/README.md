# GitHub Actions Scripts

This directory contains scripts used in the smithy-rs CI/CD workflows.

## acquire-build-image

**Purpose**: Acquires and prepares Docker build images for CI/CD workflows. Acts as an intelligent wrapper that handles
image availability checking, remote pulling from AWS ECR, authentication, and local building as needed.

### Usage

```bash
# Basic usage
./acquire-build-image

# Run self-tests
./acquire-build-image --self-test

# With environment variables
ALLOW_LOCAL_BUILD=false ./acquire-build-image
GITHUB_ACTIONS=true ./acquire-build-image
```

### Environment Variables

- `ALLOW_LOCAL_BUILD` - Enable local image building (default: `true`)
- `GITHUB_ACTIONS` - Indicates running in GitHub Actions (default: `false`)
- `ENCRYPTED_DOCKER_PASSWORD` - Base64 encrypted Docker registry password
- `DOCKER_LOGIN_TOKEN_PASSPHRASE` - Passphrase for decrypting Docker password
- `OCI_EXE` - Docker-compatible executable e.g. docker, finch, podman, etc (default: `docker`)

### Behavior & Outputs

#### When image exists locally:
- Uses existing local image
- Tags: `smithy-rs-base-image:local`, `smithy-rs-build-image:latest`
- No network activity

#### When image doesn't exist locally:
1. **Attempts remote pull** from AWS ECR (`<acccount-id>.dkr.ecr.us-west-2.amazonaws.com/smithy-rs-build-image`)
   - On success: Tags remote image as `smithy-rs-base-image:<tag>`
   - On failure: Falls back to local build (if enabled)

2. **Local build fallback** (when remote pull fails or on ARM64):
   - Builds from `tools/ci-build/Dockerfile`
   - Tags: `smithy-rs-base-image:<tag>`
   - In GitHub Actions: Also saves image to `./smithy-rs-base-image` file

3. **Final step** (always):
   - Creates user-specific build image: `smithy-rs-build-image:latest`
   - Tags base image as: `smithy-rs-base-image:local`

### Exit Codes

- `0` - Success: Build image ready for use
- `1` - Failure: Unable to acquire image

### Common Scenarios

**Local development (first run):**
```bash
./acquire-build-image
# → Pulls remote image → Tags locally → Creates build image
```

**Local development (subsequent runs):**
```bash
./acquire-build-image
# → Uses local image → Creates build image (fast)
```

**GitHub Actions:**
```bash
GITHUB_ACTIONS=true ./acquire-build-image
# → Same as above, but saves image file for job sharing if built locally
```

**ARM64 systems (Apple Silicon):**
```bash
./acquire-build-image
# → Skips remote pull → Builds locally (architecture mismatch)
```

## docker-image-hash

**Purpose**: Generates a unique hash based on the contents of the `tools/ci-build` directory. Used to create consistent Docker image tags.

### Usage

```bash
./docker-image-hash
# Outputs: a1b2c3d4e5f6... (git hash of tools/ci-build directory)
```

### Output
- Prints a git hash to stdout based on all files in `tools/ci-build`
- Hash changes only when build configuration files are modified
- Used by `acquire-build-image` to determine image tags

## upload-build-image.sh

**Purpose**: Uploads a local Docker build image to AWS ECR with proper authentication and tagging.

### Usage

```bash
# Upload with specific tag
./upload-build-image.sh <tag-name>

# Dry run (skip actual push)
DRY_RUN=true ./upload-build-image.sh <tag-name>

# Use alternative OCI executable
OCI_EXE=podman ./upload-build-image.sh <tag-name>
```

### Environment Variables
- `DRY_RUN` - Skip push to ECR (default: `false`)
- `OCI_EXE` - Docker-compatible executable (default: `docker`)

### Behavior
1. Authenticates with AWS ECR using AWS CLI
2. Tags local `smithy-rs-build-image:latest` as `<account>.dkr.ecr.us-west-2.amazonaws.com/smithy-rs-build-image:<tag>`
3. Pushes tagged image to ECR (unless `DRY_RUN=true`)

### Requirements
- AWS CLI configured with appropriate permissions
- Local image `smithy-rs-build-image:latest` must exist
