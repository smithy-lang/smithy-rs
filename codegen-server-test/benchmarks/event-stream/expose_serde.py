#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Expose private serde types only in the six generated benchmark SDKs."""
from pathlib import Path

PROJECTIONS = (
    "rest-json-legacy", "rest-json-schema", "rpc-v2-cbor-legacy",
    "rpc-v2-cbor-schema", "multi-schema", "multi-rest-json-schema",
)
FACADE = """
// Benchmark-only access to the actual generated event serde implementations.
#[doc(hidden)]
pub mod benchmark_serde {
    pub use crate::event_stream_serde::{EventsMarshaller, EventsUnmarshaller};
}
"""


def main():
    base = Path(__file__).resolve().parent.parents[1] / "build/smithyprojections/codegen-server-test"
    updates = []
    for projection in PROJECTIONS:
        source = base / f"event-stream-bench-{projection}" / "rust-server-codegen/src"
        serde = (source / "event_stream_serde.rs").read_text()
        if not all(name in serde for name in ("EventsMarshaller", "EventsUnmarshaller")):
            raise RuntimeError(f"Missing generated event serde in {source}")
        path = source / "lib.rs"
        text = path.read_text()
        if "pub mod benchmark_serde" in text and FACADE not in text:
            raise RuntimeError(f"Unexpected benchmark facade in {path}")
        if FACADE not in text:
            updates.append((path, text.rstrip() + "\n" + FACADE))
    for path, text in updates:
        path.write_text(text)
    print(f"Prepared direct serde access in {len(PROJECTIONS)} benchmark SDKs.")


if __name__ == "__main__":
    main()
