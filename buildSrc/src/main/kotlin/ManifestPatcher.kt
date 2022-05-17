/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import java.io.File

fun rewriteCrateVersion(line: String, version: String): String = line.replace(
    """^\s*version\s*=\s*"0.0.0-smithy-rs-head"$""".toRegex(),
    "version = \"$version\""
)

/**
 * AWS runtime crate versions are all `0.0.0-smithy-rs-head`. When copying over to the AWS SDK,
 * these should be changed to the AWS SDK version.
 */
fun rewriteAwsSdkCrateVersion(properties: PropertyRetriever, line: String): String =
    rewriteCrateVersion(line, properties.get("aws.sdk.version")!!)

/**
 * Smithy runtime crate versions in smithy-rs are all `0.0.0-smithy-rs-head`. When copying over to the AWS SDK,
 * these should be changed to the smithy-rs version.
 */
fun rewriteSmithyRsCrateVersion(properties: PropertyRetriever, line: String): String =
    rewriteCrateVersion(line, properties.get("smithy.rs.runtime.crate.version")!!)

/** Patches a file with the result of the given `operation` being run on each line */
fun patchFile(path: File, operation: (String) -> String) {
    val patchedContents = path.readLines().joinToString("\n", transform = operation)
    path.writeText(patchedContents)
}
