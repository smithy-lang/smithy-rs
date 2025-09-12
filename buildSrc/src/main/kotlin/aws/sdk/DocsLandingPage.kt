/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package aws.sdk

import org.gradle.api.Project
import software.amazon.smithy.utils.SimpleCodeWriter
import java.io.File

/**
 * Generate a basic docs landing page into [outputDir]
 *
 * The generated docs will include links to crates.io, docs.rs and GitHub examples for all generated services. The generated docs will
 * be written to `docs.md` in the provided [outputDir].
 */
fun Project.docsLandingPage(
    awsServices: AwsServices,
    outputPath: File,
) {
    val project = this
    val writer = SimpleCodeWriter()
    with(writer) {
        write("# AWS SDK for Rust")
        write(
            "The AWS SDK for Rust contains one crate for each AWS service, as well as ${cratesIo("aws-config")} " +
                "${docsRs("aws-config")}, a crate implementing configuration loading such as credential providers. " +
                "For usage documentation see the [Developer Guide](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/welcome.html). " +
                "For code examples refer to the [Code Examples Repository](https://github.com/awsdocs/aws-doc-sdk-examples/tree/main/rustv1).",
        )

        writer.write("## AWS Services")
        writer.write("") // empty line between header and table
        // generate a basic markdown table
        writer.write("| Service | Package |")
        writer.write("| ------- | ------- |")
        awsServices.services.sortedBy { it.humanName }.forEach {
            val items = listOfNotNull(cratesIo(it), docsRs(it)).joinToString(" ")
            writer.write(
                "| ${it.humanName} | $items |",
            )
        }
    }
    outputPath.writeText(writer.toString())
}

/**
 * Generate a link to the docs
 */
private fun docsRs(service: AwsService) = docsRs(service.crate())

private fun docsRs(crate: String) = "([docs](https://docs.rs/$crate))"

private fun cratesIo(service: AwsService) = cratesIo(service.crate())

private fun cratesIo(crate: String) = "[$crate](https://crates.io/crates/$crate)"
