/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.customizations

import com.moandjiezana.toml.TomlWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerRuntimeType
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerModuleGenerator
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

/**
 * Add `pub use aws_smithy_http_server_python::types::$TYPE` to lib.rs.
 */
class PubUsePythonTypes(private val codegenContext: ServerCodegenContext) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.Body -> writable {
                docs("Re-exported Python types from supporting crates.")
                rustBlock("pub mod python_types") {
                    rust("pub use #T;", PythonServerRuntimeType.blob(codegenContext.runtimeConfig).toSymbol())
                    rust("pub use #T;", PythonServerRuntimeType.dateTime(codegenContext.runtimeConfig).toSymbol())
                    rust("pub use #T;", PythonServerRuntimeType.document(codegenContext.runtimeConfig).toSymbol())
                }
            }
            else -> emptySection
        }
    }
}

/**
 * Render the Python shared library module export.
 */
class PythonExportModuleDecorator : ServerCodegenDecorator {
    override val name: String = "PythonExportModuleDecorator"
    override val order: Byte = 0

    override fun extras(codegenContext: ServerCodegenContext, rustCrate: RustCrate) {
        val service = codegenContext.settings.getService(codegenContext.model)
        val serviceShapes = DirectedWalker(codegenContext.model).walkShapes(service)
        PythonServerModuleGenerator(codegenContext, rustCrate, serviceShapes).render()
    }
}

/**
 * Decorator applying the customization from [PubUsePythonTypes] class.
 */
class PubUsePythonTypesDecorator : ServerCodegenDecorator {
    override val name: String = "PubUsePythonTypesDecorator"
    override val order: Byte = 0

    override fun libRsCustomizations(
        codegenContext: ServerCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUsePythonTypes(codegenContext)
    }
}

/**
 * Generates `pyproject.toml` for the crate.
 *  - Configures Maturin as the build system
 *  - Configures Python source directory
 */
class PyProjectTomlDecorator : ServerCodegenDecorator {
    override val name: String = "PyProjectTomlDecorator"
    override val order: Byte = 0

    override fun extras(codegenContext: ServerCodegenContext, rustCrate: RustCrate) {
        rustCrate.withFile("pyproject.toml") {
            val config = mapOf(
                "build-system" to listOfNotNull(
                    "requires" to listOfNotNull("maturin>=0.14,<0.15"),
                    "build-backend" to "maturin",
                ).toMap(),
                "tool" to listOfNotNull(
                    "maturin" to listOfNotNull(
                        "python-source" to "python",
                    ).toMap(),
                ).toMap(),
            )
            writeWithNoFormatting(TomlWriter().write(config))
        }
    }
}

/**
 * Adds `pyo3/extension-module` feature to default features.
 *
 * To be able to run `cargo test` with PyO3 we need two things:
 *  - Make `pyo3/extension-module` optional and default
 *  - Run tests with `cargo test --no-default-features`
 * See: https://pyo3.rs/main/faq#i-cant-run-cargo-test-or-i-cant-build-in-a-cargo-workspace-im-having-linker-issues-like-symbol-not-found-or-undefined-reference-to-_pyexc_systemerror
 */
class PyO3ExtensionModuleDecorator : ServerCodegenDecorator {
    override val name: String = "PyO3ExtensionModuleDecorator"
    override val order: Byte = 0

    override fun extras(codegenContext: ServerCodegenContext, rustCrate: RustCrate) {
        // Add `pyo3/extension-module` to default features.
        rustCrate.mergeFeature(Feature("extension-module", true, listOf("pyo3/extension-module")))
    }
}

/**
 * Generates `__init__.py` for the Python source.
 *
 * This file allows Python module to be imported like:
 * ```
 * import pokemon_service_server_sdk
 * pokemon_service_server_sdk.App()
 * ```
 * instead of:
 * ```
 * from pokemon_service_server_sdk import pokemon_service_server_sdk
 * ```
 */
class InitPyDecorator : ServerCodegenDecorator {
    override val name: String = "InitPyDecorator"
    override val order: Byte = 0

    override fun extras(codegenContext: ServerCodegenContext, rustCrate: RustCrate) {
        val libName = codegenContext.settings.moduleName.toSnakeCase()

        rustCrate.withFile("python/$libName/__init__.py") {
            writeWithNoFormatting(
                """
                from .$libName import *

                __doc__ = $libName.__doc__
                if hasattr($libName, "__all__"):
                    __all__ = $libName.__all__
                """.trimIndent(),
            )
        }
    }
}

/**
 * Generates `py.typed` for the Python source.
 *
 * This marker file is required to be PEP 561 compliant stub package.
 * Type definitions will be ignored by `mypy` if the package is not PEP 561 compliant:
 * https://mypy.readthedocs.io/en/stable/running_mypy.html#missing-library-stubs-or-py-typed-marker
 */
class PyTypedMarkerDecorator : ServerCodegenDecorator {
    override val name: String = "PyTypedMarkerDecorator"
    override val order: Byte = 0

    override fun extras(codegenContext: ServerCodegenContext, rustCrate: RustCrate) {
        val libName = codegenContext.settings.moduleName.toSnakeCase()

        rustCrate.withFile("python/$libName/py.typed") {
            writeWithNoFormatting("")
        }
    }
}

val DECORATORS = arrayOf(
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
    PythonExportModuleDecorator(),
    // Generate `pyproject.toml` for the crate.
    PyProjectTomlDecorator(),
    // Add PyO3 extension module feature.
    PyO3ExtensionModuleDecorator(),
    // Generate `__init__.py` for the Python source.
    InitPyDecorator(),
    // Generate `py.typed` for the Python source.
    PyTypedMarkerDecorator(),
)
