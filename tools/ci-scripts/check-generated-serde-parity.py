#!/usr/bin/env python3

# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Compare matching generated-serde SDKs with a mainline generation.

Generate both trees from identical models and settings (schemaSerde must be false
or omitted). Only the HTTP 1.x ServiceSerde implementation may differ in Rust
sources. Cargo manifests may differ in local runtime paths and codegen revision.
Formatting is normalized in temporary copies; neither input tree is modified.
"""

import argparse
import difflib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib


SERVICE_SERDE = re.compile(
    r"\nimpl<S> ::aws_smithy_http_server::service::ServiceSerde for (?P<service>\w+)<S> \{"
    r"\s+type SerdeKind = ::aws_smithy_http_server::response::GeneratedSerde<"
    r"\s*(?P<protocol>::[\w:]+),?\s*>;\s*\}\n"
)

MISSING_HANDLER_INDENTATION = re.compile(
    r"(self\.\w+\.unwrap_or_else\(\|\| \{\n)[ \t]+"
    r"(let svc = ::aws_smithy_(?:legacy_)?http_server::operation::MissingFailure::<[^\n]+;\n)[ \t]+"
    r"(::aws_smithy_(?:legacy_)?http_server::routing::Route::new\(svc\)\n)[ \t]+"
    r"(\}\))"
)


def format_sources(crate: Path, destination: Path) -> dict:
    shutil.copytree(crate / "src", destination)
    result = subprocess.run(
        ["rustfmt", "--edition", "2021", str(destination / "lib.rs")],
        capture_output=True,
        text=True,
    )
    # Rustfmt leaves some generated macros untouched and reports their original
    # trailing whitespace. All files, including those macros, are still compared.
    if result.returncode and "error[internal]: left behind trailing whitespace" not in result.stderr:
        raise RuntimeError(result.stderr)
    return {
        # Rustfmt retains the template's indentation in these generated arrays.
        # Normalize only whitespace around the fixed missing-handler statements.
        str(path.relative_to(destination)): MISSING_HANDLER_INDENTATION.sub(r"\1\2\3\4", path.read_text())
        for path in destination.rglob("*.rs")
    }


def remove_service_serde(source: str, reference: str) -> str:
    matches = list(SERVICE_SERDE.finditer(source))
    if len(matches) != 1:
        raise ValueError("HTTP 1.x SDK must contain exactly one generated ServiceSerde implementation")
    match = matches[0]
    service, protocol = match.group("service", "protocol")
    expected = (
        rf"impl<S> ::aws_smithy_http_server::service::ServiceShape for {service}<S> \{{"
        rf"[\s\S]*?type Protocol = {re.escape(protocol)};"
    )
    if not re.search(expected, reference):
        raise ValueError("ServiceSerde must name the mainline service and its existing protocol")
    return SERVICE_SERDE.sub("\n", source)


def manifest(crate: Path) -> dict:
    data = tomllib.loads((crate / "Cargo.toml").read_text())
    metadata = data.get("package", {}).get("metadata", {}).get("smithy", {})
    metadata.pop("codegen-version", None)
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        for name, dependency in data.get(section, {}).items():
            if isinstance(dependency, dict) and "path" in dependency and name.startswith("aws-smithy-"):
                dependency["path"] = name
    return data


def compare(reference: Path, candidate: Path, modules: list) -> int:
    failures = []
    file_count = 0
    with tempfile.TemporaryDirectory(prefix="generated-serde-parity-") as directory:
        temporary = Path(directory)
        for module in modules:
            before = reference / module / "rust-server-codegen"
            after = candidate / module / "rust-server-codegen"
            if not before.is_dir() or not after.is_dir():
                failures.append(f"{module}: missing generated SDK")
                continue
            old = format_sources(before, temporary / "reference" / module)
            new = format_sources(after, temporary / "candidate" / module)
            for name in sorted(old.keys() | new.keys()):
                if name not in old or name not in new:
                    failures.append(f"{module}/{name}: source file added or removed")
                    continue
                source = new[name]
                if name == "service.rs" and "aws-smithy-http-server" in manifest(before).get("dependencies", {}):
                    try:
                        source = remove_service_serde(source, old[name])
                    except ValueError as error:
                        failures.append(f"{module}/{name}: {error}")
                file_count += 1
                if old[name] != source:
                    failures.append(
                        "".join(
                            difflib.unified_diff(
                                old[name].splitlines(True),
                                source.splitlines(True),
                                fromfile=f"mainline/{module}/{name}",
                                tofile=f"candidate/{module}/{name}",
                            )
                        )
                    )
            old_manifest, new_manifest = manifest(before), manifest(after)
            if old_manifest != new_manifest:
                failures.append(
                    "".join(
                        difflib.unified_diff(
                            json.dumps(old_manifest, indent=2, sort_keys=True).splitlines(True),
                            json.dumps(new_manifest, indent=2, sort_keys=True).splitlines(True),
                            fromfile=f"mainline/{module}/Cargo.toml",
                            tofile=f"candidate/{module}/Cargo.toml",
                        )
                    )
                )
    for failure in failures:
        print(failure)
    if failures:
        print(f"FAILED: {len(failures)} differences across {len(modules)} SDKs")
        return 1
    print(f"PASS: {file_count} Rust files and {len(modules)} Cargo manifests; only ServiceSerde differs")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, required=True, help="mainline Smithy projections directory")
    parser.add_argument("--candidate", type=Path, required=True, help="candidate Smithy projections directory")
    parser.add_argument("--modules", required=True, help="comma-separated matching projection names")
    args = parser.parse_args()
    modules = [module.strip() for module in args.modules.split(",") if module.strip()]
    if not modules:
        parser.error("at least one projection is required")
    return compare(args.reference, args.candidate, modules)


if __name__ == "__main__":
    raise SystemExit(main())
