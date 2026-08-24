# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

import json
from pathlib import Path
from typing import Dict, Sequence, Tuple

from .commands import eprint, run
from .models import PublishedCodegen, Workspaces
from .paths import copy_file, copy_tree, exists, gradle_wrapper


PROJECT_NAME = "published-codegen-compatibility"
SMITHY_GRADLE_PLUGIN_VERSION = "1.3.0"
COMMON_MODEL_FILES = ("pokemon.smithy", "pokemon-common.smithy")
PROTOCOL_MODEL_FILE = "protocols.smithy"
MODEL_FILES = COMMON_MODEL_FILES + (PROTOCOL_MODEL_FILE,)

Protocol = Tuple[str, str, bool]

# Protocol name, service shape, and whether server codegen supports it.
COMPATIBILITY_PROTOCOLS: Tuple[Protocol, ...] = (
    ("rest-json-1", "com.aws.example#PokemonService", True),
    ("rest-xml", "smithy.rust.codegen.compatibility#RestXmlService", True),
    ("aws-json-1-0", "smithy.rust.codegen.compatibility#AwsJson10Service", True),
    ("aws-json-1-1", "smithy.rust.codegen.compatibility#AwsJson11Service", True),
    ("aws-query", "smithy.rust.codegen.compatibility#AwsQueryService", False),
    ("ec2-query", "smithy.rust.codegen.compatibility#Ec2QueryService", False),
    ("rpc-v2-cbor", "smithy.rust.codegen.compatibility#RpcV2CborService", True),
)


def _client_modules(protocols: Sequence[Protocol]) -> Tuple[str, ...]:
    return tuple(
        "protocol-{}-client".format(protocol) for protocol, _, _ in protocols
    )


def _server_modules(protocols: Sequence[Protocol]) -> Tuple[str, ...]:
    return tuple(
        "protocol-{}-server".format(protocol)
        for protocol, _, supported in protocols
        if supported
    )


def _legacy_server_modules(protocols: Sequence[Protocol]) -> Tuple[str, ...]:
    return tuple(
        "protocol-{}-server-http0x".format(protocol)
        for protocol, _, supported in protocols
        if supported
    )


CLIENT_MODULES = _client_modules(COMPATIBILITY_PROTOCOLS)
SERVER_MODULES = _server_modules(COMPATIBILITY_PROTOCOLS)
LEGACY_SERVER_MODULES = _legacy_server_modules(COMPATIBILITY_PROTOCOLS)


def generate_protocol_sdks(
    published_codegen: PublishedCodegen,
    protocols: Sequence[Protocol],
    repository_root: Path,
    destination: Path,
) -> Workspaces:
    """Generate client and supported server SDKs for the compatibility protocols.

    All projections are generated in one Gradle invocation, then copied into
    separate minimal client and server Cargo workspaces.
    """
    eprint(
        "generating protocol SDKs with published codegen {}".format(
            published_codegen.version
        )
    )
    return ProtocolSdkGenerator(repository_root).generate(
        published_codegen, protocols, destination
    )


def write_generated_workspace(workspace: Path, projection_names: Sequence[str]) -> None:
    """Create a minimal Cargo workspace around generated compatibility crates."""
    workspace.mkdir(parents=True, exist_ok=True)
    members = ['    "{}"'.format(projection) for projection in projection_names]
    (workspace / "Cargo.toml").write_text(
        '[workspace]\nresolver = "2"\nmembers = [\n{}\n]\n'.format(
            ",\n".join(members)
        )
    )


def smithy_build_config(
    protocols: Sequence[Protocol] = COMPATIBILITY_PROTOCOLS,
) -> Dict[str, object]:
    """Build client projections and supported server variants for each protocol."""
    imports = ["model/{}".format(name) for name in MODEL_FILES]
    projections: Dict[str, object] = {}
    for protocol, service, supports_server in protocols:
        common = {
            "service": service,
            "moduleVersion": "0.0.1",
            "moduleDescription": "released codegen runtime compatibility test",
            "moduleAuthors": ["protocoltest@example.com"],
        }

        client_module = "protocol-{}-client".format(protocol)
        client = dict(common)
        client.update(
            {
                "module": client_module,
                "codegen": {
                    "addMessageToErrors": True,
                    "renameErrors": True,
                    "enableNewSmithyRuntime": "orchestrator",
                },
            }
        )
        projections[client_module] = {
            "imports": imports,
            "plugins": {"rust-client-codegen": client},
        }

        if not supports_server:
            continue
        server_module = "protocol-{}-server".format(protocol)
        server = dict(common)
        server.update({"module": server_module, "codegen": {"http-1x": True}})
        projections[server_module] = {
            "imports": imports,
            "plugins": {"rust-server-codegen": server},
        }

        legacy_module = "protocol-{}-server-http0x".format(protocol)
        legacy = dict(common)
        legacy.update({"module": legacy_module, "codegen": {"http-1x": False}})
        projections[legacy_module] = {
            "imports": imports,
            "plugins": {"rust-server-codegen": legacy},
        }

    return {"version": "1.0", "projections": projections}


def gradle_build(codegen: PublishedCodegen) -> str:
    """Render an isolated Gradle build using exact published artifact versions."""
    return """plugins {{
    java
    id(\"software.amazon.smithy.gradle.smithy-base\") version \"{plugin}\"
}}

dependencies {{
    implementation(\"software.amazon.smithy.rust:codegen-client:{codegen}\")
    implementation(\"software.amazon.smithy.rust:codegen-server:{codegen}\")
    implementation(\"software.amazon.smithy:smithy-validation-model:{smithy}\")
}}

smithy {{
    format.set(false)
}}
""".format(
        plugin=SMITHY_GRADLE_PLUGIN_VERSION,
        codegen=codegen.version,
        smithy=codegen.smithy_version,
    )


class ProtocolSdkGenerator:
    """Generate protocol compatibility SDKs with published codegen JARs."""

    def __init__(self, repository_root: Path) -> None:
        self.repository_root = repository_root

    def generate(
        self,
        codegen: PublishedCodegen,
        protocols: Sequence[Protocol],
        destination: Path,
    ) -> Workspaces:
        """Generate compact client and server Cargo workspaces in one Gradle build."""
        project_root = destination / PROJECT_NAME
        self.write_gradle_and_smithy_build(project_root, codegen, protocols)
        run(
            [
                gradle_wrapper(self.repository_root),
                "-p",
                project_root,
                "smithyBuild",
                "--no-daemon",
                "--quiet",
            ],
            self.repository_root,
        )

        output_root = project_root / "build" / "smithyprojections" / PROJECT_NAME
        generated_client = destination / "generated-client"
        generated_server = destination / "generated-server"
        client_modules = _client_modules(protocols)
        server_modules = _server_modules(protocols)
        legacy_server_modules = _legacy_server_modules(protocols)
        for module in client_modules:
            self._copy_crate(
                output_root, module, "rust-client-codegen", generated_client
            )
        for module in server_modules + legacy_server_modules:
            self._copy_crate(
                output_root, module, "rust-server-codegen", generated_server
            )
        write_generated_workspace(generated_client, client_modules)
        write_generated_workspace(
            generated_server, server_modules + legacy_server_modules
        )
        return Workspaces(client=generated_client, server=generated_server)

    def write_gradle_and_smithy_build(
        self,
        project_root: Path,
        codegen: PublishedCodegen,
        protocols: Sequence[Protocol],
    ) -> None:
        """Write an isolated Smithy project using exact published artifacts."""
        model_root = project_root / "model"
        model_root.mkdir(parents=True)
        for name in COMMON_MODEL_FILES:
            source = self.repository_root / "codegen-core/common-test-models" / name
            copy_file(source, model_root / name)
        copy_file(
            Path(__file__).resolve().with_name(PROTOCOL_MODEL_FILE),
            model_root / PROTOCOL_MODEL_FILE,
        )

        (project_root / "settings.gradle.kts").write_text(
            """pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}
dependencyResolutionManagement {
    repositories { mavenCentral() }
}
rootProject.name = \"published-codegen-compatibility\"
"""
        )
        (project_root / "build.gradle.kts").write_text(gradle_build(codegen))
        (project_root / "smithy-build.json").write_text(
            json.dumps(smithy_build_config(protocols), indent=4) + "\n"
        )
        eprint(
            "prepared published codegen {} with Smithy {}".format(
                codegen.version, codegen.smithy_version
            )
        )

    @staticmethod
    def _copy_crate(
        output_root: Path,
        projection: str,
        plugin: str,
        destination_root: Path,
    ) -> None:
        """Copy one generated Rust crate and fail if codegen produced no manifest."""
        source = output_root / projection / plugin
        if not exists(source / "Cargo.toml"):
            raise RuntimeError("code generation did not create {}".format(source))
        destination_root.mkdir(parents=True, exist_ok=True)
        copy_tree(source, destination_root / projection)
