/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import org.junit.jupiter.api.Assertions
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.AllowDeprecated
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.Default
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.meta
import software.amazon.smithy.rust.codegen.core.smithy.setDefault
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.lookup
import kotlin.io.path.extension
import kotlin.io.path.readText

internal class BuilderGeneratorTest {
    private val model = StructureGeneratorTest.model
    private val inner = StructureGeneratorTest.inner
    private val struct = StructureGeneratorTest.struct
    private val credentials = StructureGeneratorTest.credentials
    private val secretStructure = StructureGeneratorTest.secretStructure
    private val errorStruct = StructureGeneratorTest.error

    @Test
    fun `generate builders`() {
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.moduleFor(inner) {
            rust("##![allow(deprecated)]")
            generator(model, provider, this, inner).render()
            generator(model, provider, this, struct).render()
            implBlock(provider.toSymbol(struct)) {
                BuilderGenerator.renderConvenienceMethod(this, provider, struct)
            }
            unitTest("generate_builders") {
                rust(
                    """
                    let my_struct_builder = MyStruct::builder().byte_value(4).foo("hello!");
                    assert_eq!(*my_struct_builder.get_byte_value(), Some(4));

                    let my_struct = my_struct_builder.build();
                    assert_eq!(my_struct.foo.unwrap(), "hello!");
                    assert_eq!(my_struct.bar, 0);
                    """,
                )
            }
        }
        project.withModule(provider.moduleForBuilder(struct)) {
            BuilderGenerator(model, provider, struct, emptyList()).render(this)
        }
        project.compileAndTest()
    }

    @Test
    fun `generate fallible builders`() {
        val baseProvider = testSymbolProvider(StructureGeneratorTest.model)
        val provider =
            object : WrappingSymbolProvider(baseProvider) {
                override fun toSymbol(shape: Shape): Symbol {
                    return baseProvider.toSymbol(shape).toBuilder().setDefault(Default.NoDefault).build()
                }
            }
        val project = TestWorkspace.testProject(provider)

        project.moduleFor(StructureGeneratorTest.struct) {
            AllowDeprecated.render(this)
            generator(model, provider, this, inner).render()
            generator(model, provider, this, struct).render()
            implBlock(provider.toSymbol(struct)) {
                BuilderGenerator.renderConvenienceMethod(this, provider, struct)
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
        project.withModule(provider.moduleForBuilder(struct)) {
            BuilderGenerator(model, provider, struct, emptyList()).render(this)
        }
        project.compileAndTest()
    }

    private fun generator(
        model: Model,
        provider: RustSymbolProvider,
        writer: RustWriter,
        shape: StructureShape,
    ) = StructureGenerator(model, provider, writer, shape, emptyList(), StructSettings(flattenVecAccessors = true))

    @Test
    fun `builder for a struct with sensitive fields should implement the debug trait as such`() {
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.moduleFor(credentials) {
            generator(model, provider, this, credentials).render()
            implBlock(provider.toSymbol(credentials)) {
                BuilderGenerator.renderConvenienceMethod(this, provider, credentials)
            }
            unitTest("sensitive_fields") {
                rust(
                    """
                    let builder = Credentials::builder()
                        .username("admin")
                        .password("pswd")
                        .secret_key("12345");
                         assert_eq!(format!("{:?}", builder),
                         "Builder { username: Some(\"admin\"), password: \"*** Sensitive Data Redacted ***\", secret_key: \"*** Sensitive Data Redacted ***\", secret_value_map: \"*** Sensitive Data Redacted ***\", secret_key_map: \"*** Sensitive Data Redacted ***\", secret_list: \"*** Sensitive Data Redacted ***\" }");
                    """,
                )
            }
        }
        project.withModule(provider.moduleForBuilder(credentials)) {
            BuilderGenerator(model, provider, credentials, emptyList()).render(this)
        }
        project.compileAndTest()
    }

    @Test
    fun `builder for a sensitive struct should implement the debug trait as such`() {
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.moduleFor(secretStructure) {
            generator(model, provider, this, secretStructure).render()
            implBlock(provider.toSymbol(secretStructure)) {
                BuilderGenerator.renderConvenienceMethod(this, provider, secretStructure)
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
        project.withModule(provider.moduleForBuilder(secretStructure)) {
            BuilderGenerator(model, provider, secretStructure, emptyList()).render(this)
        }
        project.compileAndTest()
    }

    @Test
    fun `don't add serde to error types`() {
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.moduleFor(errorStruct) {
            rust("##![allow(deprecated)]")
            StructureGenerator(model, provider, this, errorStruct, emptyList(), StructSettings(false)).render()
            implBlock(provider.toSymbol(errorStruct)) {
                BuilderGenerator.renderConvenienceMethod(this, provider, errorStruct)
            }
        }
        project.withModule(provider.moduleForBuilder(errorStruct)) {
            BuilderGenerator(model, provider, errorStruct, emptyList()).render(this)
        }
        project.compileAndTest()

        // checks if there is a serde derive in the code
        project.generatedFiles().forEach {
            if (it.extension == "rs") {
                val file = project.baseDir.resolve(it).toFile().readText()
                Assertions.assertFalse(file.contains("serde::"))
            }
        }
    }

    @Test
    fun `it supports nonzero defaults`() {
        val model =
            """
            namespace com.test
            structure MyStruct {
              @default(0)
              @required
              zeroDefault: Integer
              @required
              @default(1)
              oneDefault: OneDefault
              @required
              @default("")
              defaultEmpty: String
              @required
              @default("some-value")
              defaultValue: String
              @required
              anActuallyRequiredField: Integer
              @required
              @default([])
              emptyList: StringList
              noDefault: String
              @default(true)
              @required
              defaultDocument: Document

              @default([])
              @required
              listDocument: Document

              @default(0)
              zero_timestamp: Timestamp

              @default(1)
              one_timestamp: Timestamp

              @default("abc")
              blob_abc: Blob
            }
            list StringList {
                member: String
            }
            @default(1)
            integer OneDefault

            """.asSmithyModel(smithyVersion = "2.0")

        val provider =
            testSymbolProvider(
                model,
                rustReservedWordConfig = StructureGeneratorTest.rustReservedWordConfig,
                nullabilityCheckMode = NullableIndex.CheckMode.CLIENT_CAREFUL,
            )
        val project = TestWorkspace.testProject(provider)
        val shape: StructureShape = model.lookup("com.test#MyStruct")
        project.useShapeWriter(shape) {
            generator(model, provider, this, shape).render()
            BuilderGenerator(model, provider, shape, listOf()).render(this)
            unitTest("test_defaults") {
                rustTemplate(
                    """
                    let s = Builder::default().an_actually_required_field(5).build().unwrap();
                    assert_eq!(s.zero_default(), 0);
                    assert_eq!(s.default_empty(), "");
                    assert_eq!(s.default_value(), "some-value");
                    assert_eq!(s.one_default(), 1);
                    assert!(s.empty_list().is_empty());
                    assert_eq!(s.an_actually_required_field(), 5);
                    assert_eq!(s.no_default(), None);
                    assert_eq!(s.default_document().as_bool().unwrap(), true);
                    assert_eq!(s.list_document().as_array().expect("empty list"), &[]);
                    assert_eq!(std::str::from_utf8(s.blob_abc().as_ref()).expect("invalid blob"), "abc");
                    assert_eq!(s.zero_timestamp().secs(), 0);
                    assert_eq!(s.one_timestamp().secs(), 1);
                    """,
                    "Struct" to provider.toSymbol(shape),
                )
            }
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

        val provider = SymbolProviderWithExtraAnnotation(testSymbolProvider(model))
        val project = TestWorkspace.testProject(provider)
        project.moduleFor(inner) {
            rust("##![allow(deprecated)]")
            generator(model, provider, this, inner).render()
            implBlock(provider.toSymbol(inner)) {
                BuilderGenerator.renderConvenienceMethod(this, provider, inner)
            }
            unitTest(
                "test", additionalAttributes = listOf(Attribute.DenyDeprecated),
                block = {
                    rust(
                        // Notice that the builder is instantiated directly, and not through the Inner::builder() method.
                        // This is because Inner is marked with `deprecated`, so any usage of `Inner` inside the test will
                        // fail the compilation.
                        //
                        // This piece of code would fail though if the Builder inherits the attributes from Inner.
                        """
                        let _ = test_inner::Builder::default();
                        """,
                    )
                },
            )
        }
        project.withModule(provider.moduleForBuilder(inner)) {
            BuilderGenerator(model, provider, inner, emptyList()).render(this)
        }
        project.compileAndTest()
    }
}
