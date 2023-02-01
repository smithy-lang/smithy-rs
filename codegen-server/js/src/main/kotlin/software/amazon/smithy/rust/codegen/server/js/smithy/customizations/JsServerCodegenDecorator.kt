/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.js.smithy.customizations

import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customizations.AddInternalServerErrorToAllOperationsDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator

/**
 * Configure the [lib] section of `Cargo.toml`.
 *
 * [lib]
 * name = "$CRATE_NAME"
 * crate-type = ["cdylib"]
 */
class CdylibManifestDecorator : ServerCodegenDecorator {
    override val name: String = "CdylibDecorator"
    override val order: Byte = 0

    override fun crateManifestCustomizations(
        codegenContext: ServerCodegenContext,
    ): ManifestCustomizations =
        mapOf(
            "lib" to mapOf(
                // Library target names cannot contain hyphen names.
                "name" to codegenContext.settings.moduleName.toSnakeCase(),
                "crate-type" to listOf("cdylib"),
            ),
        )
}

val DECORATORS = listOf(
    /**
     * Add the [InternalServerError] error to all operations.
     * This is done because the Python interpreter can raise exceptions during execution.
     */
    AddInternalServerErrorToAllOperationsDecorator(),
    // Add the [lib] section to Cargo.toml to configure the generation of the shared library.
    CdylibManifestDecorator(),
    // Add `pub use` of `aws_smithy_http_server_python::types`.
)
