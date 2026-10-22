#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Generate and compile HTTP 1.x streaming blob servers with schemaSerde disabled."""
import datetime
import os
import subprocess
import sys

import run as compat


def main():
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    run = compat.SUITE / ".runs" / f"streaming-blob-http1-{stamp}"
    run.mkdir(parents=True)
    env = dict(os.environ, SCHEMA_COMPAT_ROOT=str(compat.ROOT), SCHEMA_COMPAT_RUN=str(run))
    env["SCHEMA_COMPAT_RUNTIME"] = str(compat.runtime_snapshot(run))
    env.setdefault("CARGO_TARGET_DIR", str(compat.SUITE / ".runs/target"))
    command = [str(compat.ROOT / "gradlew"), ":codegen-server:test", "--tests",
               "software.amazon.smithy.rust.codegen.server.smithy.protocols.StreamingBlobHttp1GenerationTest",
               "--rerun-tasks", "--console=plain"]
    print(f"Output: {run}", flush=True)
    with (run / "gradle.log").open("w") as log:
        result = subprocess.run(command, cwd=compat.ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    print(f"{'PASS' if result.returncode == 0 else 'FAIL'}: {run / 'gradle.log'}", flush=True)
    return result.returncode


if __name__ == "__main__":
    sys.exit(main())
