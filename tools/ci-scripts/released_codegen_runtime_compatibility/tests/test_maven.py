# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

import unittest
from unittest import mock
import xml.etree.ElementTree as ET

from released_codegen_runtime_compatibility.maven import MavenCodegenResolver
from released_codegen_runtime_compatibility.models import PublishedCodegen


METADATA = """<metadata>
  <versioning><release>0.1.24</release></versioning>
</metadata>"""
POM = """<project xmlns="http://maven.apache.org/POM/4.0.0">
  <dependencies>
    <dependency>
      <groupId>software.amazon.smithy</groupId>
      <artifactId>smithy-codegen-core</artifactId>
      <version>1.73.0</version>
    </dependency>
  </dependencies>
</project>"""


class MavenCodegenResolverTest(unittest.TestCase):
    def test_resolve_latest_release(self) -> None:
        """Resolve Maven's release marker to an exact published codegen version.
        Carry the codegen POM's Smithy version into the generation classpath.
        """
        resolver = MavenCodegenResolver()
        with mock.patch.object(
            resolver,
            "_read_xml",
            side_effect=[ET.fromstring(METADATA), ET.fromstring(POM)],
        ):
            self.assertEqual(
                PublishedCodegen("0.1.24", "1.73.0"), resolver.resolve()
            )

    def test_resolve_requested_release(self) -> None:
        """Keep an explicitly requested published codegen version unchanged.
        Skip latest-version metadata while still reading its Smithy dependency.
        """
        resolver = MavenCodegenResolver()
        with mock.patch.object(
            resolver, "_read_xml", return_value=ET.fromstring(POM)
        ) as read_xml:
            self.assertEqual(
                PublishedCodegen("0.1.20", "1.73.0"),
                resolver.resolve("0.1.20"),
            )
            self.assertEqual(1, read_xml.call_count)

    def test_reject_unsafe_version(self) -> None:
        """Reject version text that could alter generated Gradle source.
        Fail before making a Maven request for malformed caller input.
        """
        resolver = MavenCodegenResolver()
        with mock.patch.object(resolver, "_read_xml") as read_xml:
            with self.assertRaisesRegex(RuntimeError, "invalid Maven version"):
                resolver.resolve('0.1.24\")')
            read_xml.assert_not_called()


if __name__ == "__main__":
    unittest.main()
