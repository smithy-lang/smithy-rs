/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import org.jsoup.Jsoup
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.rustlang.raw
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.util.getTrait

/**
 * Generates a README.md for each service crate for display on crates.io.
 */
class AwsReadmeDecorator : RustCodegenDecorator {
    override val name: String = "AwsReadmeDecorator"
    override val order: Byte = 0

    override fun crateManifestCustomizations(codegenContext: CodegenContext): ManifestCustomizations =
        mapOf("package" to mapOf("readme" to "README.md"))

    override fun extras(codegenContext: CodegenContext, rustCrate: RustCrate) {
        rustCrate.withFile("README.md") { writer ->
            // Strip HTML from the doc trait value. In the future when it's written, we can use our Rustdoc
            // documentation normalization code to convert this to Markdown.
            val description = Jsoup.parse(
                codegenContext.settings.getService(codegenContext.model).getTrait<DocumentationTrait>()?.value ?: ""
            ).text()
            val moduleName = codegenContext.settings.moduleName

            writer.raw(
                """
                # $moduleName

                **Please Note: The SDK is currently released as an alpha and is intended strictly for
                feedback purposes only. Do not use this SDK for production workloads.**

                $description

                ## Getting Started

                > Examples are availble for many services and operations, check out the
                > [examples folder in GitHub](https://github.com/awslabs/aws-sdk-rust/tree/main/sdk/examples).

                The SDK provides one crate per AWS service. You must add [Tokio](https://crates.io/crates/tokio)
                as a dependency within your Rust project to execute asynchronous code. To add `$moduleName` to
                your project, add the following to your **Cargo.toml** file:

                ```toml
                [dependencies]
                aws-config = "${codegenContext.settings.moduleVersion}"
                $moduleName = "${codegenContext.settings.moduleVersion}"
                tokio = { version = "1", features = ["full"] }
                ```

                ## Using the SDK

                Until the SDK is released, we will be adding information about using the SDK to the
                [Guide](https://github.com/awslabs/aws-sdk-rust/blob/main/Guide.md). Feel free to suggest
                additional sections for the guide by opening an issue and describing what you are trying to do.

                ## Getting Help

                * [GitHub discussions](https://github.com/awslabs/aws-sdk-rust/discussions) - For ideas, RFCs & general questions
                * [GitHub issues](https://github.com/awslabs/aws-sdk-rust/issues/new/choose) – For bug reports & feature requests
                * [Generated Docs (latest version)](https://awslabs.github.io/aws-sdk-rust/)
                * [Usage examples](https://github.com/awslabs/aws-sdk-rust/tree/main/sdk/examples)

                ## License

                This project is licensed under the Apache-2.0 License.
                """.trimIndent()
            )
        }
    }
}
