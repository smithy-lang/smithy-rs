/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.customizations

import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerRuntimeType
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerModuleGenerator
import software.amazon.smithy.rust.codegen.server.smithy.customizations.AddInternalServerErrorToAllOperationsDecorator
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.ManifestCustomizations

/**
 * Configure the [lib] section of `Cargo.toml`.
 *
 * [lib]
 * name = "$CRATE_NAME"
 * crate-type = ["cdylib"]
 */
class CdylibManifestDecorator : RustCodegenDecorator<ServerCodegenContext> {
    override val name: String = "CdylibDecorator"
    override val order: Byte = 0

    override fun crateManifestCustomizations(
        codegenContext: ServerCodegenContext
    ): ManifestCustomizations =
        mapOf("lib" to mapOf("name" to codegenContext.settings.moduleName, "crate-type" to listOf("cdylib")))
}

/**
 * Add `pub use aws_smithy_http_server_python::types::$TYPE` to lib.rs.
 */
class PubUsePythonTypes(private val codegenContext: ServerCodegenContext) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.Body -> writable {
                docs("Re-exported Python types from supporting crates.")
                rustBlock("pub mod python_types") {
                    rust("pub use #T;", PythonServerRuntimeType.Blob(codegenContext.runtimeConfig).toSymbol())
                    rust("pub use #T;", PythonServerRuntimeType.DateTime(codegenContext.runtimeConfig).toSymbol())
                }
            }
            else -> emptySection
        }
    }
}

/**
 * Render the Python shared library module export.
 */
class PythonExportModuleDecorator : RustCodegenDecorator<ServerCodegenContext> {
    override val name: String = "PythonExportModuleDecorator"
    override val order: Byte = 0

    override fun extras(codegenContext: ServerCodegenContext, rustCrate: RustCrate) {
        val service = codegenContext.settings.getService(codegenContext.model)
        val serviceShapes = Walker(codegenContext.model).walkShapes(service)
        PythonServerModuleGenerator(codegenContext, rustCrate, serviceShapes).render()
    }
}

/**
 * Decorator applying the customization from [PubUsePythonTypes] class.
 */
class PubUsePythonTypesDecorator : RustCodegenDecorator<ServerCodegenContext> {
    override val name: String = "PubUsePythonTypesDecorator"
    override val order: Byte = 0

    override fun libRsCustomizations(
        codegenContext: ServerCodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUsePythonTypes(codegenContext)
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
    // Add `pub use` of `aws_smithy_http_server_python::types`.
    PubUsePythonTypesDecorator(),
    // Render the Python shared library export.
    PythonExportModuleDecorator()
)

// Combined codegen decorator for Python services.
class PythonServerCodegenDecorator : CombinedCodegenDecorator<ServerCodegenContext>(DECORATORS) {
    override val name: String = "PythonServerCodegenDecorator"
    override val order: Byte = -1
}
