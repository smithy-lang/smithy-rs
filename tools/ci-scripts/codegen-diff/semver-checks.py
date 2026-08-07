#!/usr/bin/env python3

#  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
#  SPDX-License-Identifier: Apache-2.0
import sys
import os
import threading
from concurrent.futures import ThreadPoolExecutor, as_completed
from diff_lib import get_cmd_output, get_cmd_status, eprint, run, run_git_commit_as_github_action, running_in_docker_build

CURRENT_BRANCH = 'current'
BASE_BRANCH = 'base'


def checkout_commit_and_generate(revision_sha, branch_name, repository_root):
    if running_in_docker_build():
        eprint(f"Fetching base revision {revision_sha} from GitHub...")
        run(f"git fetch --no-tags --progress --no-recurse-submodules --depth=1 origin {revision_sha}")

    # Generate code for HEAD
    eprint(f"Creating temporary branch {branch_name} with generated code for {revision_sha}")
    run(f"git checkout {revision_sha} -B {branch_name}")
    _generate_and_commit(revision_sha, repository_root)


def _generate_and_commit(revision_sha, repository_root):
    get_cmd_output(f"./gradlew --rerun-tasks aws:sdk:clean")
    get_cmd_output(f"./gradlew --rerun-tasks aws:sdk:assemble")

    # Remove runtime crates from the repository root to prevent `cargo-semver-checks` from failing
    # due to duplicate crates under the same root. See https://github.com/obi1kenobi/cargo-semver-checks/pull/887
    get_cmd_output(f"git rm -r {repository_root}/aws/rust-runtime")
    get_cmd_output(f"git rm -r {repository_root}/rust-runtime")

    # From this point forward, the single source of truth for the crate layout in `cargo-semver-checks`
    # is the `aws-sdk` located under the SDK's build directory.
    # However, if we commit `aws/sdk/build/aws-sdk` directly to the branch, path entries under `aws/sdk/build/`
    # will be ignored by `cargo-semver-checks`, as it uses `Walk` from the `ignore` crate.
    # https://github.com/obi1kenobi/cargo-semver-checks/blob/f55934264edbd4808fc8a7bdb9bc0350b1cc33db/src/rustdoc_gen.rs#L359
    # To address this, we need to relocate the build artifact to a directory not included in `.gitignore`,
    # and we’ve chosen the repository root arbitrarily.
    get_cmd_output(f"mv {repository_root}/aws/sdk/build/aws-sdk {repository_root}")
    get_cmd_output(f"git add -f {repository_root}/aws-sdk")

    run_git_commit_as_github_action(revision_sha)


# This script runs `cargo semver-checks` against a previous version of codegen
def main(skip_generation=False):
    if len(sys.argv) != 3:
        eprint("Usage: semver-checks.py <repository root> <base commit sha>")
        sys.exit(1)

    repository_root = sys.argv[1]
    base_commit_sha = sys.argv[2]
    os.chdir(repository_root)
    (_, head_commit_sha, _) = get_cmd_output("git rev-parse HEAD")

    # Make sure the working tree is clean
    if get_cmd_status("git diff --quiet") != 0:
        eprint("working tree is not clean. aborting")
        sys.exit(1)

    if not skip_generation:
        checkout_commit_and_generate(head_commit_sha, CURRENT_BRANCH, repository_root)
        checkout_commit_and_generate(base_commit_sha, BASE_BRANCH, repository_root)
    get_cmd_output(f'git checkout {CURRENT_BRANCH}')
    sdk_directory = os.path.join(repository_root, 'aws-sdk', 'sdk')
    os.chdir(sdk_directory)
    sdk_abs = os.path.abspath('.')

    deny_list = [
        # Proc-macro crates have no library target
        "aws-smithy-runtime-api-macros",
    ]

    # Collect the crates to check first. This part is cheap (filesystem + `git cat-file`) and stays
    # serial so the parallel section below only does the expensive `cargo semver-checks` work.
    # Absolute paths make the worker threads immune to any working-directory races.
    crates_to_check = []
    for path in sorted(os.listdir()):
        if path in deny_list:
            eprint(f"skipping {path} because it is in 'deny_list'")
        elif get_cmd_status(f'git cat-file -e base:./{path}/Cargo.toml') != 0:
            eprint(f'skipping {path} because it does not exist in base')
        elif os.path.isdir(path):
            (_, out, _) = get_cmd_output('cargo pkgid', cwd=path, quiet=True)
            pkgid = parse_package_id(out)
            crates_to_check.append((path, pkgid, os.path.join(sdk_abs, path)))

    eprint(f'{len(crates_to_check)} crates to check')
    run_semver_checks_in_parallel(crates_to_check)


# The registry that ships in the build image (index, downloaded `.crate` files, and their unpacked
# `src/`) lives at CARGO_HOME=/opt/cargo/registry. A previous attempt to parallelize this loop only
# gave each worker its own CARGO_TARGET_DIR and left CARGO_HOME shared; concurrent `cargo` processes
# then raced to unpack the same crate sources into the one shared `registry/src`, blowing up with
# "failed to unpack ... File exists (os error 17)". (cargo's `.package-cache` lock, which normally
# serializes unpacking, is ineffective here because the container runs as the host uid against a
# `build`-owned CARGO_HOME.)
#
# The fix is to give each worker a fully private CARGO_HOME so no two workers ever touch the same
# mutable path — for both the head and baseline dependency sets. The large, read-only pieces (the
# registry index, the downloaded `.crate` cache, and the git db) are symlinked in so nothing is
# copied; only the per-worker mutable dirs (`registry/src` where crates get unpacked, and
# `.package-cache`) are private and writable. A worker's home is reused across the crates it checks,
# so incremental compilation still benefits.
SEMVER_MAX_WORKERS = min(4, os.cpu_count() or 4)
_worker_local = threading.local()


def _worker_env():
    """Return an env dict with a private, race-free CARGO_HOME + CARGO_TARGET_DIR for this thread."""
    env = getattr(_worker_local, 'env', None)
    if env is not None:
        return env

    shared_cargo_home = os.environ.get('CARGO_HOME', '/opt/cargo')
    worker_root = os.path.join(
        os.environ.get('TMPDIR', '/tmp'),
        f'semver-worker-{threading.get_ident()}',
    )
    cargo_home = os.path.join(worker_root, 'cargo-home')
    os.makedirs(os.path.join(cargo_home, 'registry'), exist_ok=True)

    # Symlink the large read-only pieces from the shared registry (no copying); keep the dirs that
    # cargo mutates during a check (`registry/src`, `.package-cache`) private to this worker.
    for rel in ('registry/index', 'registry/cache', 'git'):
        src = os.path.join(shared_cargo_home, rel)
        if os.path.exists(src):
            dst = os.path.join(cargo_home, rel)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            if not os.path.lexists(dst):
                os.symlink(src, dst)
    # Preserve the cargo config (registry mirrors, credentials, etc.) if the image sets one.
    for config_name in ('config.toml', 'config'):
        src = os.path.join(shared_cargo_home, config_name)
        dst = os.path.join(cargo_home, config_name)
        if os.path.exists(src) and not os.path.lexists(dst):
            os.symlink(src, dst)

    env = {
        **os.environ,
        'CARGO_HOME': cargo_home,
        'CARGO_TARGET_DIR': os.path.join(worker_root, 'target'),
    }
    _worker_local.env = env
    return env


def _check_crate(item):
    path, pkgid, abs_path = item
    manifest = os.path.join(abs_path, 'Cargo.toml')
    (status, out, err) = get_cmd_output(
        f'cargo semver-checks check-release '
        f'--baseline-rev {BASE_BRANCH} '
        # in order to get semver-checks to work with publish-false crates, need to specify
        # package and manifest path explicitly
        f'--manifest-path {manifest} '
        '-v '
        f'-p {pkgid} '
        f'--all-features '
        f'--release-type minor',
        check=False, quiet=True, env=_worker_env())
    return (path, status, out, err)


def run_semver_checks_in_parallel(crates_to_check):
    failures = []
    with ThreadPoolExecutor(max_workers=SEMVER_MAX_WORKERS) as executor:
        futures = {executor.submit(_check_crate, c): c for c in crates_to_check}
        for future in as_completed(futures):
            path, status, out, err = future.result()
            if status == 0:
                eprint(f'checking {path}...ok!')
            else:
                eprint(f'checking {path}...failed!')
                if out:
                    eprint(out)
                eprint(err)
                failures.append(f"{out}{err}")
    if failures:
        eprint('One or more crates failed semver checks!')
        eprint("\n".join(failures))
        exit(1)


def parse_package_id(id):
    if '#' in id and '@' in id:
        return id.split('#')[1].split('@')[0]
    elif '#' in id:
        return id.split('/')[-1].split('#')[0]
    else:
        eprint(id)
        raise Exception("unknown format")


import unittest


class SelfTest(unittest.TestCase):
    def test_foo(self):
        self.assertEqual(parse_package_id("file:///Users/rcoh/code/smithy-rs-ci/smithy-rs/tmp-codegen-diff/aws-sdk/sdk/aws-smithy-runtime-api#0.56.1"), "aws-smithy-runtime-api")
        self.assertEqual(parse_package_id("file:///Users/rcoh/code/smithy-rs-ci/smithy-rs/tmp-codegen-diff/aws-sdk/sdk/s3#aws-sdk-s3@0.0.0-local"), "aws-sdk-s3")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        sys.argv.pop()
        unittest.main()
    else:
        skip_generation = bool(os.environ.get('SKIP_GENERATION') or False)
        main(skip_generation=skip_generation)
