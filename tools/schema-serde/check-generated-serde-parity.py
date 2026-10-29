#!/usr/bin/env python3

# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

"""Generate and compare server SDKs from mainline and a schema-serde checkout.

With no arguments, prompt for both repositories and a model (default: pokemon).
With --suite, generate every case in a suite file instead (default suite:
parity-suite.json next to this script). Both generations use identical models
and settings with schemaSerde disabled, in an HTTP 1.x and an HTTP 0.x SDK per
case. If both repositories are the same checkout, generate mainline in a
temporary detached worktree at origin/main (override with --mainline-ref).

Rust sources are compared as token streams, so whitespace and non-doc comments
are ignored. Only the HTTP 1.x ServiceSerde implementation may differ. Cargo
manifests may differ in local runtime paths and codegen revision; all other
files must be identical. Generation logs and SDKs are saved in the printed
output directory. Existing projection trees can still be compared using
--reference, --candidate, and --modules.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
import difflib
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import tomllib


TOOL = Path(__file__).resolve().parent
TOKENS_MANIFEST = TOOL / "rust-tokens" / "Cargo.toml"
DEFAULT_SUITE = TOOL / "parity-suite.json"
HTTP0X = "-http0x"
MAX_HUNK_LINES = 40


def tokens_pattern(*parts: str) -> str:
    """Joins token patterns the way token streams are joined: one token per line."""
    return "\n".join(parts)


def literal(text: str) -> str:
    """Pattern for a space-separated sequence of literal tokens."""
    return tokens_pattern(*(re.escape(token) for token in text.split()))


SERVICE_SERDE = re.compile(
    r"(?:^|\n)"
    + tokens_pattern(
        literal("impl < S > :: aws_smithy_http_server :: service :: ServiceSerde for"),
        r"(?P<service>\w+)",
        literal("< S > { type SerdeKind = :: aws_smithy_http_server :: response :: GeneratedSerde <"),
        r"(?P<protocol>::\n\w+(?:\n::\n\w+)*)",
        r"(?:,\n)?>",
        literal("; }"),
    )
    + r"(?=\n|$)"
)


def rust_tokens(files: list[Path]) -> dict[Path, list[tuple[int, str]]]:
    """Returns the (line, token) stream of each Rust file."""
    if not files:
        return {}
    result = subprocess.run(
        ["cargo", "run", "--quiet", "--release", "--manifest-path", str(TOKENS_MANIFEST), "--"]
        + [str(file) for file in files],
        capture_output=True,
        text=True,
    )
    if result.returncode:
        raise RuntimeError(f"rust-tokens failed: {result.stderr}")
    streams = {}
    current = None
    for line in result.stdout.splitlines():
        number, token = line.split("\t", 1)
        if number == "file":
            current = streams[Path(token)] = []
        else:
            current.append((int(number), token))
    return streams


def remove_service_serde(source: list[tuple[int, str]], reference: list[tuple[int, str]]) -> list[tuple[int, str]]:
    joined = "\n".join(token for _, token in source)
    matches = list(SERVICE_SERDE.finditer(joined))
    if len(matches) != 1:
        raise ValueError("HTTP 1.x SDK must contain exactly one generated ServiceSerde implementation")
    match = matches[0]
    service, protocol = match.group("service", "protocol")
    expected = (
        tokens_pattern(
            literal("impl < S > :: aws_smithy_http_server :: service :: ServiceShape for"),
            re.escape(service),
            literal("< S > {"),
        )
        + r"[\s\S]*?\n"
        + tokens_pattern(literal("type Protocol ="), re.escape(protocol), literal(";"))
    )
    if not re.search(expected, "\n".join(token for _, token in reference)):
        raise ValueError("ServiceSerde must name the mainline service and its existing protocol")
    # Each token is one line of the joined stream; a leading separator belongs to the match.
    start = joined.count("\n", 0, match.start()) + (1 if match.group().startswith("\n") else 0)
    length = match.group().strip("\n").count("\n") + 1
    return source[:start] + source[start + length:]


def token_diff(module: str, name: str, before: Path, after: Path, old: list, new: list) -> str:
    """Shows the source lines around each differing token run."""
    old_lines = before.read_text().splitlines()
    new_lines = after.read_text().splitlines()
    matcher = difflib.SequenceMatcher(None, [t for _, t in old], [t for _, t in new], autojunk=False)
    hunks = [f"--- mainline/{module}/{name}\n+++ candidate/{module}/{name}\n"]
    for tag, i1, i2, j1, j2 in matcher.get_opcodes():
        if tag == "equal":
            continue

        def span(stream: list, start: int, end: int) -> tuple[int, int]:
            if start < end:
                return stream[start][0], stream[end - 1][0]
            # Nothing on this side: an empty range after the preceding token's line.
            line = stream[start - 1][0] if start else 0
            return line + 1, line

        a1, a2 = span(old, i1, i2)
        b1, b2 = span(new, j1, j2)
        hunks.append(f"@@ tokens {tag}: mainline lines {a1}-{a2}, candidate lines {b1}-{b2} @@\n")
        for prefix, lines, first, last in (("-", old_lines, a1, a2), ("+", new_lines, b1, b2)):
            shown = lines[max(first - 1, 0):last][:MAX_HUNK_LINES]
            hunks.extend(f"{prefix}{line}\n" for line in shown)
            if last - first + 1 > MAX_HUNK_LINES:
                hunks.append(f"{prefix}... {last - first + 1 - MAX_HUNK_LINES} more lines\n")
    return "".join(hunks)


def manifest(crate: Path) -> dict:
    data = tomllib.loads((crate / "Cargo.toml").read_text())
    metadata = data.get("package", {}).get("metadata", {}).get("smithy", {})
    metadata.pop("codegen-version", None)
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        for name, dependency in data.get(section, {}).items():
            if isinstance(dependency, dict) and "path" in dependency and name.startswith("aws-smithy-"):
                dependency["path"] = name
    return data


def crate_files(crate: Path) -> dict[str, Path]:
    return {
        str(path.relative_to(crate)): path
        for path in crate.rglob("*")
        if path.is_file() and "target" not in path.relative_to(crate).parts
    }


def compare_module(reference: Path, candidate: Path, module: str) -> tuple[list[str], int]:
    before = reference / module / "rust-server-codegen"
    after = candidate / module / "rust-server-codegen"
    if not before.is_dir() or not after.is_dir():
        return [f"{module}: missing generated SDK"], 0
    failures = []
    old_files, new_files = crate_files(before), crate_files(after)
    for name in sorted(old_files.keys() ^ new_files.keys()):
        failures.append(f"{module}/{name}: file added or removed")
    shared = sorted(old_files.keys() & new_files.keys())
    rust = [name for name in shared if name.endswith(".rs")]
    streams = rust_tokens([old_files[name] for name in rust] + [new_files[name] for name in rust])
    is_http1x = "aws-smithy-http-server" in manifest(before).get("dependencies", {})
    for name in rust:
        old, new = streams[old_files[name]], streams[new_files[name]]
        if name == "src/service.rs" and is_http1x and "ServiceSerde" not in {t for _, t in old}:
            try:
                new = remove_service_serde(new, old)
            except ValueError as error:
                failures.append(f"{module}/{name}: {error}")
        if [t for _, t in old] != [t for _, t in new]:
            failures.append(token_diff(module, name, old_files[name], new_files[name], old, new))
    for name in shared:
        if name.endswith(".rs") or name == "Cargo.toml":
            continue
        if old_files[name].read_bytes() != new_files[name].read_bytes():
            failures.append(f"{module}/{name}: contents differ")
    old_manifest, new_manifest = manifest(before), manifest(after)
    # Test-only dependencies the candidate adds so generated protocol tests compile
    # (mainline can emit tests that use an undeclared crate) are reported, not failed.
    old_dev = old_manifest.get("dev-dependencies", {})
    new_dev = new_manifest.get("dev-dependencies", {})
    added = sorted(set(new_dev) - set(old_dev))
    if added:
        print(f"note: {module}/Cargo.toml adds dev-dependencies: {', '.join(added)}")
        for name in added:
            del new_dev[name]
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
    return failures, len(rust)


def compare(reference: Path, candidate: Path, modules: list) -> int:
    failed_modules = []
    file_count = 0
    for module in modules:
        failures, count = compare_module(reference, candidate, module)
        file_count += count
        for failure in failures:
            print(failure)
        status = "FAIL" if failures else "ok"
        print(f"[{status}] {module}: {count} Rust files, {len(failures)} differences", flush=True)
        if failures:
            failed_modules.append(module)
    if failed_modules:
        print(f"FAILED: {len(failed_modules)} of {len(modules)} SDKs differ: {', '.join(failed_modules)}")
        return 1
    print(f"PASS: {file_count} Rust files and {len(modules)} SDKs; only ServiceSerde differs")
    return 0


def prompt(label: str, default: str) -> str:
    return input(f"{label} [{default}]: ").strip() or default


def repository(path: Path) -> Path:
    path = path.expanduser().resolve()
    if not (path / "gradlew").is_file() or not (path / "codegen-server").is_dir():
        raise ValueError(f"Not a smithy-rs repository: {path}")
    return path


def model_inputs(model: str | None, repo: Path, service: str | None) -> tuple[list[Path], str]:
    if model is None:
        # Services from the protocol test jars are discovered on the classpath.
        return [], service
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


def codegen_settings(settings: dict, origin: str) -> dict:
    if not isinstance(settings, dict) or settings.keys() & {"http-1x", "schemaSerde"}:
        raise ValueError(f"{origin}: codegen settings must be a JSON object without http-1x or schemaSerde")
    return settings


def suite_cases(path: Path, repo: Path, names: str | None) -> list[dict]:
    cases = json.loads(path.read_text())["cases"]
    if names:
        wanted = [name.strip() for name in names.split(",") if name.strip()]
        unknown = set(wanted) - {case["name"] for case in cases}
        if unknown:
            raise ValueError(f"Unknown suite cases: {', '.join(sorted(unknown))}")
        cases = [case for case in cases if case["name"] in wanted]
    for case in cases:
        if case["name"].endswith(HTTP0X):
            raise ValueError(f"Suite case names must not end with {HTTP0X}: {case['name']}")
        case["models"] = [repo / model for model in case.get("models", [])]
        for model in case["models"]:
            if not model.exists():
                raise ValueError(f"{case['name']}: model does not exist: {model}")
        case["codegen"] = codegen_settings(case.get("codegen", {}), case["name"])
    return cases


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


def modules_for(cases: list[dict]) -> list[str]:
    return [module for case in cases for module in (case["name"], case["name"] + HTTP0X)]


def generate(repo: Path, label: str, output: Path, cases: list[dict]) -> None:
    projections = {}
    for case in cases:
        for module in (case["name"], case["name"] + HTTP0X):
            codegen = dict(case["codegen"])
            if not module.endswith(HTTP0X):
                codegen["http-1x"] = True
            if label == "schema-serde":
                codegen["schemaSerde"] = False
            projections[module] = {
                "imports": [str(model) for model in case["models"]],
                "plugins": {
                    "rust-server-codegen": {
                        "runtimeConfig": {"relativePath": str(repo / "rust-runtime")},
                        "codegen": codegen,
                        "service": case["service"],
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
        # Smithy still writes the projections that succeeded; the comparison
        # reports the missing ones.
        failed = re.findall(r"Projection (\S+) failed", log.read_text())
        print(f"WARNING: {label} generation failed for {', '.join(failed) or 'the build'}; see {log}", flush=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mainline-repo", type=Path, help="mainline repository checkout")
    parser.add_argument("--schema-serde-repo", type=Path, help="schema-serde repository checkout")
    parser.add_argument("--model", help="pokemon, a model file/directory, or comma-separated model files")
    parser.add_argument("--service", help="Smithy service ID (inferred for a model with one service; "
                        "without --model, a service from the protocol test jars)")
    parser.add_argument("--codegen", default="{}", help="extra codegen settings as a JSON object, applied to both generations")
    parser.add_argument("--suite", nargs="?", const=DEFAULT_SUITE, type=Path,
                        help=f"compare every case in a suite file (default: {DEFAULT_SUITE.name})")
    parser.add_argument("--cases", help="comma-separated suite case names to run")
    parser.add_argument("--mainline-ref", default="origin/main", help="mainline revision when both repositories are the same")
    parser.add_argument("--output", type=Path, help="directory for generated SDKs and logs (default: a new temporary directory)")
    parser.add_argument("--reference", type=Path, help="existing mainline Smithy projections directory")
    parser.add_argument("--candidate", type=Path, help="existing candidate Smithy projections directory")
    parser.add_argument("--modules", help="comma-separated matching projection names for existing trees")
    args = parser.parse_args()
    if any((args.reference, args.candidate, args.modules)):
        if not all((args.reference, args.candidate, args.modules)):
            parser.error("--reference, --candidate, and --modules must be supplied together")
        if any((args.mainline_repo, args.schema_serde_repo, args.model, args.service, args.output, args.suite)):
            parser.error("choose repository generation or existing projection comparison")
        modules = [module.strip() for module in args.modules.split(",") if module.strip()]
        if not modules:
            parser.error("at least one projection is required")
        return compare(args.reference, args.candidate, modules)
    if args.suite and any((args.model, args.service, args.codegen != "{}")):
        parser.error("--suite cases define their own models, services, and codegen settings")
    if args.cases and not args.suite:
        parser.error("--cases requires --suite")
    try:
        default_repo = TOOL.parents[1]
        mainline = repository(args.mainline_repo or Path(prompt("Mainline repository", str(default_repo))))
        candidate = repository(args.schema_serde_repo or Path(prompt("Schema-serde repository", str(default_repo))))
        if args.suite:
            cases = suite_cases(args.suite, candidate, args.cases)
        else:
            interactive = args.mainline_repo is None or args.schema_serde_repo is None or sys.stdin.isatty()
            model = args.model
            if model is None and args.service is None:
                model = prompt("Model (pokemon or model path)", "pokemon") if interactive else "pokemon"
            models, service = model_inputs(model, candidate, args.service)
            settings = codegen_settings(json.loads(args.codegen), "--codegen")
            cases = [{"name": "serde-parity", "service": service, "models": models, "codegen": settings}]
        output = args.output.expanduser().resolve() if args.output else Path(tempfile.mkdtemp(prefix="generated-serde-parity-run-"))
        output.mkdir(parents=True, exist_ok=True)
        for name in ("mainline", "schema-serde"):
            if (output / name).exists():
                raise ValueError(f"Output already contains {name}: {output}")
        (output / "generation.gradle").write_text(GENERATION_TASK)
        print(f"SDKs and logs: {output}", flush=True)
        with mainline_checkout(mainline, candidate, args.mainline_ref) as reference:
            # Distinct checkouts, so both Gradle builds can run at once.
            with ThreadPoolExecutor(max_workers=2) as pool:
                generations = [
                    pool.submit(generate, reference, "mainline", output, cases),
                    pool.submit(generate, candidate, "schema-serde", output, cases),
                ]
                for generation in generations:
                    generation.result()
        return compare(output / "mainline", output / "schema-serde", modules_for(cases))
    except (EOFError, ValueError, RuntimeError, OSError, subprocess.CalledProcessError) as error:
        print(f"ERROR: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
