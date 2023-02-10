/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.AllowDeprecated
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.Default
import software.amazon.smithy.rust.codegen.core.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.core.smithy.setDefault
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.PANIC

internal class BuilderGeneratorTest {
    private val model = StructureGeneratorTest.model
    private val inner = StructureGeneratorTest.inner
    private val struct = StructureGeneratorTest.struct
    private val credentials = StructureGeneratorTest.credentials
    private val secretStructure = StructureGeneratorTest.secretStructure

    @Test
    fun `generate builders`() {
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.moduleFor(inner) {
            rust("##![allow(deprecated)]")
            StructureGenerator(model, provider, this, inner).render()
            StructureGenerator(model, provider, this, struct).render()
            BuilderGenerator(model, provider, struct).also { builderGen ->
                builderGen.render(this)
                implBlock(struct, provider) {
                    builderGen.renderConvenienceMethod(this)
                }
            }

            unitTest("generate_builders") {
                rust(
                    """
                    let my_struct = MyStruct::builder().byte_value(4).foo("hello!").build();
                    assert_eq!(my_struct.foo.unwrap(), "hello!");
                    assert_eq!(my_struct.bar, 0);
                    """,
                )
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `generate fallible builders`() {
        val baseProvider = testSymbolProvider(StructureGeneratorTest.model)
        val provider =
            object : RustSymbolProvider {
                override fun config(): SymbolVisitorConfig {
                    return baseProvider.config()
                }

                override fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed? {
                    return baseProvider.toEnumVariantName(definition)
                }

                override fun toSymbol(shape: Shape?): Symbol {
                    return baseProvider.toSymbol(shape).toBuilder().setDefault(Default.NoDefault).build()
                }

                override fun symbolForOperationError(operation: OperationShape): Symbol = PANIC()
                override fun symbolForEventStreamError(eventStream: UnionShape): Symbol = PANIC()

                override fun toMemberName(shape: MemberShape?): String {
                    return baseProvider.toMemberName(shape)
                }
            }
        val project = TestWorkspace.testProject(provider)
        project.moduleFor(StructureGeneratorTest.struct) {
            AllowDeprecated.render(this)
            StructureGenerator(model, provider, this, inner).render()
            StructureGenerator(model, provider, this, struct).render()
            BuilderGenerator(model, provider, struct).also { builderGenerator ->
                builderGenerator.render(this)
                implBlock(struct, provider) {
                    builderGenerator.renderConvenienceMethod(this)
                }
            }
            unitTest("generate_fallible_builders") {
                rust(
                    """
                    let my_struct = MyStruct::builder().byte_value(4).foo("hello!").bar(0).build().expect("required field was not provided");
                    assert_eq!(my_struct.foo.unwrap(), "hello!");
                    assert_eq!(my_struct.bar, 0);
                    """,
                )
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `builder for a struct with sensitive fields should implement the debug trait as such`() {
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.moduleFor(credentials) {
            StructureGenerator(model, provider, this, credentials).render()
            BuilderGenerator(model, provider, credentials).also { builderGen ->
                builderGen.render(this)
                implBlock(credentials, provider) {
                    builderGen.renderConvenienceMethod(this)
                }
            }
            unitTest("sensitive_fields") {
                rust(
                    """
                    let builder = Credentials::builder()
                        .username("admin")
                        .password("pswd")
                        .secret_key("12345");
                         assert_eq!(format!("{:?}", builder), "Builder { username: Some(\"admin\"), password: \"*** Sensitive Data Redacted ***\", secret_key: \"*** Sensitive Data Redacted ***\" }");
                    """,
                )
            }
        }
        project.compileAndTest()
    }

    @Test
    fun `builder for a sensitive struct should implement the debug trait as such`() {
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.moduleFor(secretStructure) {
            StructureGenerator(model, provider, this, secretStructure).render()
            BuilderGenerator(model, provider, secretStructure).also { builderGen ->
                builderGen.render(this)
                implBlock(secretStructure, provider) {
                    builderGen.renderConvenienceMethod(this)
                }
            }
            unitTest("sensitive_struct") {
                rust(
                    """
                    let builder = SecretStructure::builder()
                        .secret_field("secret");
                    assert_eq!(format!("{:?}", builder), "Builder { secret_field: \"*** Sensitive Data Redacted ***\" }");
                    """,
                )
            }
        }
        project.compileAndTest()
    }
}
