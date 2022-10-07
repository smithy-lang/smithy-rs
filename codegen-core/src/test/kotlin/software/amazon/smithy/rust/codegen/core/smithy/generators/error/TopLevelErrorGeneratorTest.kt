/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators.error

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.testutil.DefaultTestPublicModules
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.util.runCommand
import kotlin.io.path.ExperimentalPathApi
import kotlin.io.path.createDirectory
import kotlin.io.path.writeText

internal class TopLevelErrorGeneratorTest {
    @ExperimentalPathApi
    @Test
    fun `top level errors are send + sync`() {
        val model = """
            namespace com.example

            use aws.protocols#restJson1

            @restJson1
            service HelloService {
                operations: [SayHello],
                version: "1"
            }

            @http(uri: "/", method: "POST")
            operation SayHello {
                input: EmptyStruct,
                output: EmptyStruct,
                errors: [SorryBusy, CanYouRepeatThat, MeDeprecated]
            }

            structure EmptyStruct { }

            @error("server")
            structure SorryBusy { }

            @error("client")
            structure CanYouRepeatThat { }

            @error("client")
            @deprecated
            structure MeDeprecated { }
        """.asSmithyModel()

        val (pluginContext, testDir) = generatePluginContext(model)
        val moduleName = pluginContext.settings.expectStringMember("module").value.replace('-', '_')
        val symbolProvider = testSymbolProvider(model)
        val settings = CoreRustSettings.from(model, pluginContext.settings)
        val codegenContext = CodegenContext(
            model,
            symbolProvider,
            model.expectShape(ShapeId.from("com.example#HelloService")) as ServiceShape,
            ShapeId.from("aws.protocols#restJson1"),
            settings,
            CodegenTarget.CLIENT,
        )

        val rustCrate = RustCrate(
            pluginContext.fileManifest,
            symbolProvider,
            DefaultTestPublicModules,
            codegenContext.settings.codegenConfig,
        )

        rustCrate.lib { writer ->
            writer.rust("##![allow(deprecated)]")
        }
        rustCrate.withModule(RustModule.Error) { writer ->
            for (shape in model.structureShapes) {
                if (shape.id.namespace == "com.example") {
                    StructureGenerator(model, symbolProvider, writer, shape).render(CodegenTarget.CLIENT)
                }
            }
        }
        TopLevelErrorGenerator(codegenContext, model.operationShapes.toList()).render(rustCrate)

        testDir.resolve("tests").createDirectory()
        testDir.resolve("tests/validate_errors.rs").writeText(
            """
            fn check_send_sync<T: Send + Sync>() {}
            #[test]
            fn tl_errors_are_send_sync() {
                check_send_sync::<$moduleName::Error>()
            }
            """,
        )
        rustCrate.finalize(settings, model, emptyMap(), emptyList(), false)

        "cargo test".runCommand(testDir)
    }
}
