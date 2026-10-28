#!/usr/bin/env python3

# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Generate and compare server SDKs from mainline and a schema-serde checkout.

With no arguments, prompt for both repositories and a model (default: pokemon).
Both generations use identical models with schemaSerde disabled. If both
repositories are the same checkout, generate mainline in a temporary detached
worktree at origin/main (override with --mainline-ref).

Only the HTTP 1.x ServiceSerde implementation may differ in Rust
sources. Cargo manifests may differ in local runtime paths and codegen revision.
Formatting is normalized in temporary copies. Generation logs and SDKs are saved
in the printed output directory. Existing projection trees can still be compared
using --reference, --candidate, and --modules.
"""

import argparse
from contextlib import contextmanager
import difflib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
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
                if (
                    name == "service.rs"
                    and "aws-smithy-http-server" in manifest(before).get("dependencies", {})
                    and SERVICE_SERDE.search(source)
                    and not SERVICE_SERDE.search(old[name])
                ):
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


def prompt(label: str, default: str) -> str:
    return input(f"{label} [{default}]: ").strip() or default


def repository(path: Path) -> Path:
    path = path.expanduser().resolve()
    if not (path / "gradlew").is_file() or not (path / "codegen-server").is_dir():
        raise ValueError(f"Not a smithy-rs repository: {path}")
    return path


def model_inputs(model: str, repo: Path, service: str | None) -> tuple[list[Path], str]:
    if model.lower() == "pokemon":
        models = [
            repo / "codegen-core/common-test-models/pokemon.smithy",
            repo / "codegen-core/common-test-models/pokemon-common.smithy",
        ]
        return models, service or "com.aws.example#PokemonService"
    models = [Path(name.strip()).expanduser().resolve() for name in model.split(",")]
    services = set()
    for path in models:
        if not path.exists():
            raise ValueError(f"Model does not exist: {path}")
        files = sorted(path.rglob("*.smithy")) + sorted(path.rglob("*.json")) if path.is_dir() else [path]
        for file in files:
            if file.suffix == ".smithy":
                source = re.sub(r"/\*[\s\S]*?\*/|//[^\n]*", "", file.read_text())
                namespace = re.search(r"(?m)^\s*namespace\s+([\w.]+)", source)
                if namespace:
                    services.update(
                        f"{namespace.group(1)}#{name}"
                        for name in re.findall(r"(?m)^\s*service\s+(\w+)\s*\{", source)
                    )
            elif file.suffix == ".json":
                data = json.loads(file.read_text())
                services.update(
                    name for name, shape in data.get("shapes", {}).items() if shape.get("type") == "service"
                )
    if service:
        return models, service
    if len(services) != 1:
        raise ValueError(f"Specify --service with a Smithy service ID; found: {', '.join(sorted(services)) or 'none'}")
    return models, services.pop()


@contextmanager
def mainline_checkout(mainline: Path, candidate: Path, revision: str):
    if mainline != candidate:
        yield mainline
        return
    # Separate outputs alone would still run the same codegen twice. Use a
    # detached worktree so the caller's branch, index, and working files survive.
    with tempfile.TemporaryDirectory(prefix="serde-parity-mainline-") as directory:
        checkout = Path(directory) / "repo"
        subprocess.run(
            ["git", "-C", str(mainline), "worktree", "add", "--detach", str(checkout), revision],
            check=True,
        )
        try:
            yield checkout
        finally:
            subprocess.run(
                ["git", "-C", str(mainline), "worktree", "remove", str(checkout)], check=True
            )


GENERATION_TASK = """
allprojects {
    afterEvaluate { project ->
        if (project.path == ':codegen-server-test') {
            def parityCli = project.configurations.create('generatedSerdeParityCli')
            project.dependencies.add('generatedSerdeParityCli', "software.amazon.smithy:smithy-cli:${System.getProperty('paritySmithyVersion')}")
            project.tasks.register('generateSerdeParity', JavaExec) {
                dependsOn(':codegen-server:jar')
                classpath = project.configurations.runtimeClasspath + parityCli
                mainClass = 'software.amazon.smithy.cli.SmithyCli'
                args 'build', '--discover', '--config', System.getProperty('parityConfig'), '--output', System.getProperty('parityOutput')
            }
        }
    }
}
"""


def generate(repo: Path, label: str, output: Path, models: list[Path], service: str) -> list[str]:
    modules = ["serde-parity", "serde-parity-http0x"]
    projections = {}
    for module in modules:
        codegen = {"http-1x": True} if module == "serde-parity" else {}
        if label == "schema-serde":
            codegen["schemaSerde"] = False
        projections[module] = {
            "imports": [str(model) for model in models],
            "plugins": {
                "rust-server-codegen": {
                    "runtimeConfig": {"relativePath": str(repo / "rust-runtime")},
                    "codegen": codegen,
                    "service": service,
                    "module": module,
                    "moduleVersion": "0.0.1",
                    "moduleDescription": "generated serde parity",
                    "moduleAuthors": ["protocoltest@example.com"],
                }
            },
        }
    config = output / f"{label}.json"
    config.write_text(json.dumps({"version": "1.0", "projections": projections}, indent=2) + "\n")
    versions = tomllib.loads((repo / "gradle/libs.versions.toml").read_text())
    revision = subprocess.check_output(["git", "-C", str(repo), "rev-parse", "HEAD"], text=True).strip()
    print(f"Generating {label} at {revision} ({repo})", flush=True)
    log = output / f"{label}-generation.log"
    with log.open("w") as stream:
        result = subprocess.run(
            [
                str(repo / "gradlew"), "--quiet", "-I", str(output / "generation.gradle"),
                f"-DparityConfig={config}", f"-DparityOutput={output / label}",
                f"-DparitySmithyVersion={versions['versions']['smithy-version']}",
                ":codegen-server-test:generateSerdeParity",
            ],
            cwd=repo, stdout=stream, stderr=subprocess.STDOUT,
        )
    if result.returncode:
        raise RuntimeError(f"{label} generation failed; see {log}")
    return modules


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mainline-repo", type=Path, help="mainline repository checkout")
    parser.add_argument("--schema-serde-repo", type=Path, help="schema-serde repository checkout")
    parser.add_argument("--model", help="pokemon, a model file/directory, or comma-separated model files")
    parser.add_argument("--service", help="Smithy service ID (inferred for a model with one service)")
    parser.add_argument("--mainline-ref", default="origin/main", help="mainline revision when both repositories are the same")
    parser.add_argument("--output", type=Path, help="directory for generated SDKs and logs (default: a new temporary directory)")
    parser.add_argument("--reference", type=Path, help="existing mainline Smithy projections directory")
    parser.add_argument("--candidate", type=Path, help="existing candidate Smithy projections directory")
    parser.add_argument("--modules", help="comma-separated matching projection names for existing trees")
    args = parser.parse_args()
    if any((args.reference, args.candidate, args.modules)):
        if not all((args.reference, args.candidate, args.modules)):
            parser.error("--reference, --candidate, and --modules must be supplied together")
        if any((args.mainline_repo, args.schema_serde_repo, args.model, args.service, args.output)):
            parser.error("choose repository generation or existing projection comparison")
        modules = [module.strip() for module in args.modules.split(",") if module.strip()]
        if not modules:
            parser.error("at least one projection is required")
        return compare(args.reference, args.candidate, modules)
    try:
        default_repo = Path(__file__).resolve().parents[2]
        mainline = repository(args.mainline_repo or Path(prompt("Mainline repository", str(default_repo))))
        candidate = repository(args.schema_serde_repo or Path(prompt("Schema-serde repository", str(default_repo))))
        interactive = args.mainline_repo is None or args.schema_serde_repo is None or sys.stdin.isatty()
        model = args.model or (prompt("Model (pokemon or model path)", "pokemon") if interactive else "pokemon")
        models, service = model_inputs(model, candidate, args.service)
        output = args.output.expanduser().resolve() if args.output else Path(tempfile.mkdtemp(prefix="generated-serde-parity-run-"))
        output.mkdir(parents=True, exist_ok=True)
        for name in ("mainline", "schema-serde"):
            if (output / name).exists():
                raise ValueError(f"Output already contains {name}: {output}")
        (output / "generation.gradle").write_text(GENERATION_TASK)
        print(f"SDKs and logs: {output}", flush=True)
        with mainline_checkout(mainline, candidate, args.mainline_ref) as reference:
            generate(reference, "mainline", output, models, service)
        modules = generate(candidate, "schema-serde", output, models, service)
        return compare(output / "mainline", output / "schema-serde", modules)
    except (EOFError, ValueError, RuntimeError, OSError, subprocess.CalledProcessError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
