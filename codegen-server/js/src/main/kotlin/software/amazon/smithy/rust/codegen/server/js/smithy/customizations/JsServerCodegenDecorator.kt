/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.js.smithy.customizations

import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.js.smithy.JsServerCargoDependency
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

class NapiBuildRsDecorator : ServerCodegenDecorator {
    override val name: String = "NapiBuildRsDecorator"
    override val order: Byte = 0
    private val napi_build = JsServerCargoDependency.NapiBuild.toType()

    override fun extras(codegenContext: ServerCodegenContext, rustCrate: RustCrate) {
        rustCrate.withFile("build.rs") {
            rustTemplate(
                """
                fn main() {
                    #{napi_build}::setup();
                }
                """,
                "napi_build" to napi_build,
            )
        }
    }
}

val DECORATORS = listOf(
    /**
     * Add the [InternalServerError] error to all operations.
     * This is done because the Python interpreter can raise exceptions during execution.
     */
    AddInternalServerErrorToAllOperationsDecorator(),
    // Add the [lib] section to Cargo.toml to configure the generation of the shared library.
    CdylibManifestDecorator(),
    //
    NapiBuildRsDecorator(),
)
