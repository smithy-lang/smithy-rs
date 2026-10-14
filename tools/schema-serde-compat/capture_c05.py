#!/usr/bin/env python3
"""Freshly generate C05 servers and save actual loopback HTTP/1.1 bytes."""
import datetime
import json
import os
import select
import socket
import subprocess
import tomllib

import run_c05 as compat


def main():
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    run = compat.SUITE / ".runs" / f"C05-wire-{stamp}"
    run.mkdir(parents=True)
    env = dict(os.environ, SCHEMA_COMPAT_ROOT=str(compat.ROOT), SCHEMA_COMPAT_RUN=str(run))
    env["SCHEMA_COMPAT_RUNTIME"] = str(compat.runtime_snapshot(run))
    env["CARGO_TARGET_DIR"] = str(compat.SUITE / ".runs/target")
    sources = [compat.KOTLIN_TEST, compat.SUITE / "models/error-content-type.smithy",
               compat.SUITE / "capture_c05.py", compat.SUITE / "capture_c05.rs.in", compat.SUITE / "run_c05.py"]
    for directory in ["codegen-core/src/main", "codegen-server/src/main", "codegen-traits/src/main", "rust-runtime"]:
        sources.extend(compat.source_files(compat.ROOT / directory))
    (run / "source-fingerprints.json").write_text(json.dumps(compat.fingerprints(sources), indent=2) + "\n")
    print(f"Wire capture: {run}", flush=True)
    with (run / "gradle.log").open("w") as log:
        result = compat.run_logged([str(compat.ROOT / "gradlew"), ":codegen-server:test", "--tests",
                                 "software.amazon.smithy.rust.codegen.server.smithy.protocols.ModeledErrorContentTypeCompatibilityTest",
                                 "--rerun-tasks", "--console=plain", "--info"], cwd=compat.ROOT, env=env, log=log)
    # Allow capturing a discrepancy before a fix as well as matching responses
    # afterward. Require every captured server to finish its fresh observation test.
    for service in ["JsonErrorContentType", "XmlErrorContentType"]:
        for mode in ["legacy", "schema"]:
            observations = run / f"{service}-{mode}.json"
            if not observations.exists() or len(json.loads(observations.read_text())) != 42:
                raise RuntimeError(f"Incomplete generation: {observations}; Gradle exit {result.returncode}")
    env["RUSTFLAGS"] = "-D warnings -A non-snake-case"
    for service in ["JsonErrorContentType", "XmlErrorContentType"]:
        for mode in ["legacy", "schema"]:
            crate = run / f"{service}-{mode}"
            manifest = crate / "Cargo.toml"
            package = tomllib.loads(manifest.read_text())["package"]["name"].replace("-", "_")
            with manifest.open("a") as f:
                f.write('\n[dev-dependencies.hyper-util]\nversion = "0.1"\nfeatures = ["tokio"]\n')
            examples = crate / "examples"
            examples.mkdir(exist_ok=True)
            source = (compat.SUITE / "capture_c05.rs.in").read_text().replace("__CRATE__", package).replace("__SERVICE__", service)
            (examples / "c05_wire.rs").write_text(source)
            with (run / f"{service}-{mode}-wire-build.log").open("w") as log:
                compat.run_logged(["cargo", "build", "--verbose", "--example", "c05_wire"], cwd=crate, env=env,
                               log=log, check=True)
            with (run / f"{service}-{mode}-wire-server.log").open("w") as log:
                server = subprocess.Popen([str(compat.SUITE / ".runs/target/debug/examples/c05_wire")],
                                          cwd=crate, env=env, stdout=subprocess.PIPE, stderr=log, text=True)
                try:
                    if not select.select([server.stdout], [], [], 20)[0]:
                        raise RuntimeError("Server did not report its listening address")
                    address = server.stdout.readline().strip()
                    host, port = address.rsplit(":", 1)
                    for operation in ["MediaText", "MediaBlob", "Document"]:
                        request = f"GET /{operation} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n".encode()
                        with socket.create_connection((host, int(port)), timeout=10) as connection:
                            connection.sendall(request)
                            chunks = []
                            while data := connection.recv(65536):
                                chunks.append(data)
                        response = b"".join(chunks)
                        assert response.startswith(b"HTTP/1.1 422 "), response
                        prefix = run / f"{service}-{mode}-{operation}"
                        prefix.with_suffix(".request.http").write_bytes(request)
                        prefix.with_suffix(".response.http").write_bytes(response)
                        print(f"{service}/{mode}/{operation}:\n{response.decode()}\n", flush=True)
                    assert server.wait(timeout=10) == 0
                finally:
                    if server.poll() is None:
                        server.terminate()
                        server.wait(timeout=10)
    print(f"Raw HTTP captures saved in {run}", flush=True)


if __name__ == "__main__":
    main()
