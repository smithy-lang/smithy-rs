/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * The set of artifacts published to Maven Central under `software.amazon.smithy.rust`, and the
 * source paths that determine their contents.
 *
 * [tasks.CheckMavenCentralPublishingNeeded] uses this to answer two questions that both depend on
 * knowing exactly what gets published and what changes it: "is there anything to publish?" and "did
 * a published artifact change without `codegenVersion` being bumped?".
 *
 * To confirm the list is complete: every project that applies `smithy-rs.publishing-conventions`
 * publishes, and nothing else does. Verified against repo1.maven.org — all eight artifactIds below
 * return 200 for their `maven-metadata.xml`, and `fuzzgen` returns 404.
 */
object PublishedMavenArtifacts {
    /**
     * An artifact and the source paths whose contents end up inside it.
     *
     * [sourcePaths] are repository-relative prefixes suitable for `git diff -- <path>`. They are
     * deliberately narrower than the Gradle project directory where only part of a project is
     * packaged, so that a change which cannot affect the artifact does not demand a version bump.
     */
    data class Artifact(
        val artifactId: String,
        val sourcePaths: List<String>,
    )

    /** The Maven group, as a repository path segment. */
    const val GROUP_PATH = "software/amazon/smithy/rust"

    /**
     * Paths that feed every artifact: the convention plugins that configure compilation and
     * packaging, the crate set that decides what gets copied into the runtime jars, and the
     * dependency version catalog. A change to any of these can alter every jar.
     *
     * This list is deliberately narrower than all of `buildSrc/`. Most build logic there — the
     * codegen test harness, the Rust build-tool wrappers, this file — cannot change a published
     * jar's contents, and treating it as if it could would demand a version bump on routine build
     * changes. Anything that does shape the artifacts belongs here explicitly.
     *
     * `gradle.properties` is deliberately absent: it holds `codegenVersion` itself, so including it
     * would make "something changed" true on every version bump and the check vacuous.
     */
    val SHARED_SOURCE_PATHS =
        listOf(
            "buildSrc/src/main/kotlin/smithy-rs.publishing-conventions.gradle.kts",
            "buildSrc/src/main/kotlin/smithy-rs.kotlin-conventions.gradle.kts",
            // Decides which runtime crates are copied into the rust-runtime jar.
            "buildSrc/src/main/kotlin/CrateSet.kt",
            "gradle/libs.versions.toml",
        )

    val ARTIFACTS =
        listOf(
            Artifact("codegen-core", listOf("codegen-core")),
            Artifact("codegen-client", listOf("codegen-client")),
            Artifact("codegen-server", listOf("codegen-server")),
            Artifact("codegen-serde", listOf("codegen-serde")),
            Artifact("codegen-traits", listOf("codegen-traits")),
            Artifact("codegen-aws-sdk", listOf("aws/codegen-aws-sdk")),
            // Only `inlineable/` is packaged into these two jars; the rest of the runtime
            // directory ships to crates.io instead. See rust-runtime/build.gradle.kts and
            // aws/rust-runtime/build.gradle.kts.
            Artifact("rust-runtime", listOf("rust-runtime/inlineable")),
            Artifact("aws-rust-runtime", listOf("aws/rust-runtime/aws-inlineable")),
        )

    val artifactIds: List<String> get() = ARTIFACTS.map { it.artifactId }

    /** Every path that can affect a published artifact, including the shared build logic. */
    val allSourcePaths: List<String> get() = (ARTIFACTS.flatMap { it.sourcePaths } + SHARED_SOURCE_PATHS).distinct()

    /** The POM URL for an artifact at a given version, used to test whether it is published. */
    fun pomUrl(
        artifactId: String,
        version: String,
    ): String = "https://repo1.maven.org/maven2/$GROUP_PATH/$artifactId/$version/$artifactId-$version.pom"

    /** The `maven-metadata.xml` URL for an artifact, which lists all published versions. */
    fun metadataUrl(artifactId: String): String =
        "https://repo1.maven.org/maven2/$GROUP_PATH/$artifactId/maven-metadata.xml"
}
