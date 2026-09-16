# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

import re
from typing import Optional
from urllib.error import URLError
from urllib.parse import quote
from urllib.request import urlopen
import xml.etree.ElementTree as ET

from .models import PublishedCodegen


MAVEN_CENTRAL = "https://repo1.maven.org/maven2"
CODEGEN_GROUP_PATH = "software/amazon/smithy/rust"
VERSION_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+-]*$")


class MavenCodegenResolver:
    """Resolve smithy-rs codegen releases from their published Maven metadata.
    Return exact versions so client, server, and Smithy model artifacts stay aligned.
    """

    def __init__(self, repository_url: str = MAVEN_CENTRAL) -> None:
        """Store the Maven repository used for metadata and POM resolution.
        Allow tests or mirrors to replace Maven Central without changing generation.
        """
        self.repository_url = repository_url.rstrip("/")

    def resolve(self, requested_version: Optional[str] = None) -> PublishedCodegen:
        """Resolve an exact codegen version and its Smithy dependency version.
        Select Maven's latest release when the caller does not request a version.
        """
        version = requested_version or self._latest_version()
        self._validate_version(version)
        smithy_version = self._smithy_version(version)
        self._validate_version(smithy_version)
        return PublishedCodegen(version=version, smithy_version=smithy_version)

    def _latest_version(self) -> str:
        """Read the latest released codegen-client version from Maven metadata.
        Reject incomplete metadata instead of allowing a dynamic Gradle dependency.
        """
        root = self._read_xml(
            "{}/{}/codegen-client/maven-metadata.xml".format(
                self.repository_url, CODEGEN_GROUP_PATH
            )
        )
        release = root.findtext("./versioning/release")
        if not release:
            raise RuntimeError("Maven metadata does not contain a codegen release")
        return release.strip()

    def _smithy_version(self, codegen_version: str) -> str:
        """Read the Smithy codegen-core version used by a published client JAR.
        Use that exact version for model dependencies loaded during generation.
        """
        encoded = quote(codegen_version, safe="")
        root = self._read_xml(
            "{0}/{1}/codegen-client/{2}/codegen-client-{2}.pom".format(
                self.repository_url, CODEGEN_GROUP_PATH, encoded
            )
        )
        namespace = {"m": "http://maven.apache.org/POM/4.0.0"}
        for dependency in root.findall("./m:dependencies/m:dependency", namespace):
            group = dependency.findtext("m:groupId", namespaces=namespace)
            artifact = dependency.findtext("m:artifactId", namespaces=namespace)
            if group == "software.amazon.smithy" and artifact == "smithy-codegen-core":
                version = dependency.findtext("m:version", namespaces=namespace)
                if version:
                    return version.strip()
        raise RuntimeError(
            "codegen-client {} POM does not declare smithy-codegen-core".format(
                codegen_version
            )
        )

    def _read_xml(self, url: str) -> ET.Element:
        """Download and parse one Maven XML document with a bounded timeout.
        Convert network and malformed-document failures into actionable errors.
        """
        try:
            with urlopen(url, timeout=30) as response:
                return ET.fromstring(response.read())
        except (URLError, OSError, ET.ParseError) as error:
            raise RuntimeError("failed to read Maven metadata from {}: {}".format(url, error))

    @staticmethod
    def _validate_version(version: str) -> None:
        """Require a Maven version safe for URLs and generated Gradle source.
        Reject empty or syntactically surprising values before writing build files.
        """
        if not VERSION_PATTERN.match(version):
            raise RuntimeError("invalid Maven version: {!r}".format(version))


def resolve_published_codegen(
    requested_version: Optional[str] = None,
) -> PublishedCodegen:
    """Resolve a requested codegen release, or Maven Central's latest release."""
    return MavenCodegenResolver().resolve(requested_version)
