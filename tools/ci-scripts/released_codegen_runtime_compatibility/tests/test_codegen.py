# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

import unittest

from released_codegen_runtime_compatibility.codegen import (
    CLIENT_MODULES,
    LEGACY_SERVER_MODULES,
    COMPATIBILITY_PROTOCOLS,
    SERVER_MODULES,
    gradle_build,
    smithy_build_config,
)
from released_codegen_runtime_compatibility.models import PublishedCodegen


class PublishedCodegenProjectTest(unittest.TestCase):
    def test_smithy_build_covers_every_supported_protocol(self) -> None:
        """Generate every client protocol and both HTTP stacks for server protocols.
        Ensure no projection redirects generated dependencies to local runtimes.
        """
        projections = smithy_build_config()["projections"]
        expected = set(CLIENT_MODULES + SERVER_MODULES + LEGACY_SERVER_MODULES)
        self.assertEqual(expected, set(projections))
        self.assertEqual(7, len(CLIENT_MODULES))
        self.assertEqual(5, len(SERVER_MODULES))
        self.assertEqual(5, len(LEGACY_SERVER_MODULES))
        self.assertEqual(
            {
                "rest-json-1",
                "rest-xml",
                "aws-json-1-0",
                "aws-json-1-1",
                "aws-query",
                "ec2-query",
                "rpc-v2-cbor",
            },
            {protocol for protocol, _, _ in COMPATIBILITY_PROTOCOLS},
        )

        rest_json = projections["protocol-rest-json-1-client"]["plugins"][
            "rust-client-codegen"
        ]
        self.assertEqual(
            "smithy.rust.codegen.compatibility#RestJsonService",
            rest_json["service"],
        )

        for module in CLIENT_MODULES:
            settings = projections[module]["plugins"]["rust-client-codegen"]
            self.assertNotIn("runtimeConfig", settings)
        for module in SERVER_MODULES:
            settings = projections[module]["plugins"]["rust-server-codegen"]
            self.assertNotIn("runtimeConfig", settings)
            self.assertTrue(settings["codegen"]["http-1x"])
        for module in LEGACY_SERVER_MODULES:
            settings = projections[module]["plugins"]["rust-server-codegen"]
            self.assertNotIn("runtimeConfig", settings)
            self.assertFalse(settings["codegen"]["http-1x"])

    def test_gradle_build_uses_published_jars(self) -> None:
        """Verify generation loads exact published client and server artifacts.
        Ensure the current checkout's codegen projects cannot enter the classpath.
        """
        build = gradle_build(
            PublishedCodegen(version="0.1.24", smithy_version="1.73.0")
        )
        self.assertIn(
            "software.amazon.smithy.rust:codegen-client:0.1.24", build
        )
        self.assertIn(
            "software.amazon.smithy.rust:codegen-server:0.1.24", build
        )
        self.assertIn(
            "software.amazon.smithy:smithy-validation-model:1.73.0", build
        )
        self.assertNotIn("project(", build)


if __name__ == "__main__":
    unittest.main()
