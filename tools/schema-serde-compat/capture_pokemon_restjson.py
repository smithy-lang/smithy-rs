#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Run the mainline Pokémon REST JSON example and capture an actual HTTP/1.1 response."""
import argparse
import datetime
import json
import selectors
import socket
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkout", type=Path, default=Path.home() / "kaam/smithy-rs-restJson")
    args = parser.parse_args()
    example = args.checkout / "examples/pokemon-restjson-wire"
    run = Path(__file__).resolve().parent / ".runs" / (
        "pokemon-restjson-wire-" + datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    )
    run.mkdir(parents=True)
    with (run / "build.log").open("w") as log:
        subprocess.run(["cargo", "build", "--quiet", "--manifest-path", str(example / "Cargo.toml")],
                       stdout=log, stderr=subprocess.STDOUT, check=True)
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--quiet", "--no-deps", "--format-version", "1",
         "--manifest-path", str(example / "Cargo.toml")], text=True))
    binary = Path(metadata["target_directory"]) / "debug/pokemon-restjson-wire"
    with (run / "server.log").open("w") as errors:
        server = subprocess.Popen([str(binary)], stdout=subprocess.PIPE, stderr=errors, text=True)
        try:
            with selectors.DefaultSelector() as selector:
                selector.register(server.stdout, selectors.EVENT_READ)
                if not selector.select(30):
                    raise RuntimeError("Server did not become ready")
            ready = server.stdout.readline().strip()
            if not ready.startswith("READY "):
                raise RuntimeError(f"Unexpected server output: {ready}")
            port = int(ready.split()[1])
            request = (
                "GET /radio HTTP/1.1\r\n"
                f"Host: 127.0.0.1:{port}\r\n"
                "Accept: application/octet-stream\r\n"
                "Content-Length: 0\r\n"
                "Connection: close\r\n\r\n"
            ).encode("ascii")
            (run / "request.http").write_bytes(request)
            with socket.create_connection(("127.0.0.1", port), timeout=10) as connection:
                connection.sendall(request)
                chunks = []
                while chunk := connection.recv(65536):
                    chunks.append(chunk)
            response = b"".join(chunks)
            (run / "response.http").write_bytes(response)
            head, body = response.split(b"\r\n\r\n", 1)
            assert head.startswith(b"HTTP/1.1 200"), head
            assert b"content-type: application/octet-stream" in head.lower(), head
            if b"transfer-encoding: chunked" in head.lower():
                remaining = body
                chunks = []
                while True:
                    size_line, remaining = remaining.split(b"\r\n", 1)
                    size = int(size_line.split(b";", 1)[0], 16)
                    if size == 0:
                        break
                    chunks.append(remaining[:size])
                    assert remaining[size:size + 2] == b"\r\n"
                    remaining = remaining[size + 2:]
                body = b"".join(chunks)
            (run / "response.body").write_bytes(body)
            assert body == b"hello", body.hex()
            print(head.decode("ascii"))
            print(f"Body hex: {body.hex()}")
            print(f"Capture: {run}")
        finally:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()


if __name__ == "__main__":
    main()
