/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.meta
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerRestJsonProtocol
import software.amazon.smithy.rust.codegen.server.smithy.renderInlineMemoryModules
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext

class ServerBuilderGeneratorTest {
    @Test
    fun `it respects the sensitive trait in Debug impl`() {
        val model =
            """
            namespace test
            @sensitive
            string SecretKey

            @sensitive
            string Password

            structure Credentials {
                username: String,
                password: Password,
                secretKey: SecretKey
            }
            """.asSmithyModel()

        val codegenContext = serverTestCodegenContext(model)
        val project = TestWorkspace.testProject()
        project.withModule(ServerRustModule.Model) {
            val writer = this
            val shape = model.lookup<StructureShape>("test#Credentials")

            StructureGenerator(model, codegenContext.symbolProvider, writer, shape, emptyList(), codegenContext.structSettings()).render()
            val builderGenerator =
                ServerBuilderGenerator(
                    codegenContext,
                    shape,
                    SmithyValidationExceptionConversionGenerator(codegenContext),
                    ServerRestJsonProtocol(codegenContext),
                )

            builderGenerator.render(project, writer)

            writer.implBlock(codegenContext.symbolProvider.toSymbol(shape)) {
                builderGenerator.renderConvenienceMethod(this)
            }

            project.renderInlineMemoryModules()
        }

        project.unitTest {
            rust(
                """
                use super::*;
                use crate::model::*;
                let builder = Credentials::builder()
                    .username(Some("admin".to_owned()))
                    .password(Some("pswd".to_owned()))
                    .secret_key(Some("12345".to_owned()));
                     assert_eq!(format!("{:?}", builder), "Builder { username: Some(\"admin\"), password: \"*** Sensitive Data Redacted ***\", secret_key: \"*** Sensitive Data Redacted ***\" }");
                """,
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `builder doesn't inherit attributes from struct`() {
        /**
         * This test checks if the generated Builder doesn't inherit the macro attributes added to the main struct.
         *
         * The strategy is to:
         * 1) mark the `Inner` struct with `#[deprecated]`
         * 2) deny use of deprecated in the test
         * 3) allow use of deprecated by the Builder
         * 4) Ensure that the builder can be instantiated
         */
        val model =
            """
            namespace test

            structure Inner {}
            """.asSmithyModel()

        class SymbolProviderWithExtraAnnotation(val base: RustSymbolProvider) : RustSymbolProvider by base {
            override fun toSymbol(shape: Shape): Symbol {
                val baseSymbol = base.toSymbol(shape)
                val name = baseSymbol.name
                if (name == "Inner") {
                    var metadata = baseSymbol.expectRustMetadata()
                    val attribute = Attribute.Deprecated
                    metadata = metadata.copy(additionalAttributes = metadata.additionalAttributes + listOf(attribute))
                    return baseSymbol.toBuilder().meta(metadata).build()
                } else {
                    return baseSymbol
                }
            }
        }

        val codegenContext = serverTestCodegenContext(model)
        val provider = SymbolProviderWithExtraAnnotation(codegenContext.symbolProvider)
        val project = TestWorkspace.testProject(provider)
        project.withModule(ServerRustModule.Model) {
            val shape = model.lookup<StructureShape>("test#Inner")
            val writer = this

            rust("##![allow(deprecated)]")
            StructureGenerator(model, provider, writer, shape, emptyList(), codegenContext.structSettings()).render()
            val builderGenerator =
                ServerBuilderGenerator(
                    codegenContext,
                    shape,
                    SmithyValidationExceptionConversionGenerator(codegenContext),
                    ServerRestJsonProtocol(codegenContext),
                )

            builderGenerator.render(project, writer)

            writer.implBlock(provider.toSymbol(shape)) {
                builderGenerator.renderConvenienceMethod(this)
            }

            project.renderInlineMemoryModules()
        }
        project.unitTest(additionalAttributes = listOf(Attribute.DenyDeprecated), test = {
            rust(
                // Notice that the builder is instantiated directly, and not through the Inner::builder() method.
                // This is because Inner is marked with `deprecated`, so any usage of `Inner` inside the test will
                // fail the compilation.
                //
                // This piece of code would fail though if the Builder inherits the attributes from Inner.
                """
                let _ = crate::model::inner::Builder::default();
                """,
            )
        })
        project.compileAndTest()
    }
}
