/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy.customizations

import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customizations.AddInternalServerErrorToAllOperationsDecorator
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.typescript.smithy.TsServerCargoDependency

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

    override fun crateManifestCustomizations(codegenContext: ServerCodegenContext): ManifestCustomizations =
        mapOf(
            "lib" to
                mapOf(
                    "crate-type" to listOf("cdylib"),
                ),
        )
}

class NapiBuildRsDecorator : ServerCodegenDecorator {
    override val name: String = "NapiBuildRsDecorator"
    override val order: Byte = 0
    private val napi_build = TsServerCargoDependency.NapiBuild.toType()

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) {
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

class NapiPackageJsonDecorator : ServerCodegenDecorator {
    override val name: String = "NapiPackageJsonDecorator"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ServerCodegenContext,
        rustCrate: RustCrate,
    ) {
        val name = codegenContext.settings.moduleName.toSnakeCase()
        val version = codegenContext.settings.moduleVersion

        // TODO(https://github.com/smithy-lang/smithy-rs/issues/2317): we should probably use a real JSON writer, but I did not want to add
        //  other external libraries at this stage.
        rustCrate.withFile("package.json") {
            val content = """{
                "name": "@amzn/$name",
                "version": "$version",
                "main": "index.js",
                "types": "index.d.ts",
                "napi": {
                    "name": "$name",
                    "triple": {}
                },
                "devDependencies": {
                    "@napi-rs/cli": ">=2",
                    "@types/node": ">=18"
                },
                "engines": {
                    "node": ">=18"
                },
                "scripts": {
                    "artifacts": "napi artifacts",
                    "build": "napi build --platform --release",
                    "build:debug": "napi build --platform",
                    "prepublishOnly": "napi prepublish -t npm",
                    "universal": "napi universal",
                    "version": "napi version"
                },
                "packageManager": "yarn",
                "dependencies": {
                    "yarn": ">=1"
                }
}"""
            this.write(content)
        }
    }
}

val DECORATORS =
    arrayOf(
        /*
         * Add the [InternalServerError] error to all operations.
         * This is done because the Typescript interpreter can raise eceptions during execution.
         */
        AddInternalServerErrorToAllOperationsDecorator(),
        // Add the [lib] section to Cargo.toml to configure the generation of the shared library.
        CdylibManifestDecorator(),
        // Add the build.rs file needed to generate Typescript code.
        NapiBuildRsDecorator(),
        // Add the napi package.json.
        NapiPackageJsonDecorator(),
    )
