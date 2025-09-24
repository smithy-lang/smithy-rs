/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.CsvSource
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator.Companion.hasFallibleBuilder
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.TestEnumType
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.lookup

class AwsQuerySerializerGeneratorTest {
    private val baseModel =
        """
        namespace test
        use aws.protocols#restJson1

        union Choice {
            blob: Blob,
            boolean: Boolean,
            date: Timestamp,
            enum: FooEnum,
            int: Integer,
            @xmlFlattened
            list: SomeList,
            long: Long,
            map: MyMap,
            number: Double,
            s: String,
            top: Top,
            unit: Unit,
        }

        @enum([{name: "FOO", value: "FOO"}])
        string FooEnum

        map MyMap {
            key: String,
            value: Choice,
        }

        list SomeList {
            member: Choice
        }

        structure Top {
            choice: Choice,
            field: String,
            extra: Long,
            @xmlName("rec")
            recursive: TopList
        }

        list TopList {
            @xmlName("item")
            member: Top
        }

        structure OpInput {
            @xmlName("some_bool")
            boolean: Boolean,
            list: SomeList,
            map: MyMap,
            top: Top,
        }

        @http(uri: "/top", method: "POST")
        operation Op {
            input: OpInput,
        }
        """.asSmithyModel()

    @ParameterizedTest
    @CsvSource("true", "false")
    fun `generates valid serializers`(generateUnknownVariant: Boolean) {
        val codegenTarget =
            when (generateUnknownVariant) {
                true -> CodegenTarget.CLIENT
                false -> CodegenTarget.SERVER
            }
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(baseModel))
        val codegenContext = testCodegenContext(model, codegenTarget = codegenTarget)
        val symbolProvider = codegenContext.symbolProvider
        val parserGenerator = AwsQuerySerializerGenerator(codegenContext)
        val operationGenerator = parserGenerator.operationInputSerializer(model.lookup("test#Op"))

        val project = TestWorkspace.testProject(symbolProvider)
        project.lib {
            unitTest(
                "query_serializer",
                """
                use test_model::Top;

                let input = crate::test_input::OpInput::builder()
                    .top(
                        Top::builder()
                            .field("hello!")
                            .extra(45)
                            .recursive(Top::builder().extra(55).build())
                            .build()
                    )
                    .boolean(true)
                    .build()
                    .unwrap();
                let serialized = ${format(operationGenerator!!)}(&input).unwrap();
                let output = std::str::from_utf8(serialized.bytes().unwrap()).unwrap();
                assert_eq!(
                    output,
                    "\
                    Action=Op\
                    &Version=test\
                    &some_bool=true\
                    &top.field=hello%21\
                    &top.extra=45\
                    &top.rec.item.1.extra=55\
                    "
                );
                """,
            )
        }
        model.lookup<StructureShape>("test#Top").also { top ->
            top.renderWithModelBuilder(model, symbolProvider, project)
            project.moduleFor(top) {
                UnionGenerator(
                    model,
                    symbolProvider,
                    this,
                    model.lookup("test#Choice"),
                    renderUnknownVariant = generateUnknownVariant,
                ).render()
                val enum = model.lookup<StringShape>("test#FooEnum")
                EnumGenerator(model, symbolProvider, enum, TestEnumType, emptyList()).render(this)
            }
        }

        model.lookup<OperationShape>("test#Op").inputShape(model).also { input ->
            input.renderWithModelBuilder(model, symbolProvider, project)
        }
        project.compileAndTest()
    }

    private val baseModelWithRequiredTypes =
        """
        namespace test
        use aws.protocols#restJson1

        union Choice {
            blob: Blob,
            boolean: Boolean,
            date: Timestamp,
            enum: FooEnum,
            int: Integer,
            @xmlFlattened
            list: SomeList,
            long: Long,
            map: MyMap,
            number: Double,
            s: String,
            top: Top,
            unit: Unit,
        }

        @enum([{name: "FOO", value: "FOO"}])
        string FooEnum

        map MyMap {
            key: String,
            value: Choice,
        }

        list SomeList {
            member: Choice
        }

        structure Top {
            @required
            choice: Choice,
            @required
            field: String,
            @required
            extra: Long,
            @xmlName("rec")
            recursive: TopList
        }

        list TopList {
            @xmlName("item")
            member: Top
        }

        structure OpInput {
            @required
            @xmlName("some_bool")
            boolean: Boolean,
            list: SomeList,
            map: MyMap,
            @required
            top: Top,
            @required
            blob: Blob
        }

        @http(uri: "/top", method: "POST")
        operation Op {
            input: OpInput,
        }
        """.asSmithyModel()

    @ParameterizedTest
    @CsvSource(
        "true, CLIENT",
        "true, CLIENT_CAREFUL",
        "true, CLIENT_ZERO_VALUE_V1",
        "true, CLIENT_ZERO_VALUE_V1_NO_INPUT",
        "false, SERVER",
    )
    fun `generates valid serializers for required types`(
        generateUnknownVariant: Boolean,
        nullabilityCheckMode: NullableIndex.CheckMode,
    ) {
        val codegenTarget =
            when (generateUnknownVariant) {
                true -> CodegenTarget.CLIENT
                false -> CodegenTarget.SERVER
            }
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(baseModelWithRequiredTypes))
        val codegenContext =
            testCodegenContext(model, codegenTarget = codegenTarget, nullabilityCheckMode = nullabilityCheckMode)
        val symbolProvider = codegenContext.symbolProvider
        val parserGenerator = AwsQuerySerializerGenerator(codegenContext)
        val operationGenerator = parserGenerator.operationInputSerializer(model.lookup("test#Op"))

        val project = TestWorkspace.testProject(symbolProvider)

        // Depending on the nullability check mode, the builder can be fallible or not. When it's fallible, we need to
        // add unwrap calls.
        val builderIsFallible = hasFallibleBuilder(model.lookup<StructureShape>("test#Top"), symbolProvider)
        val maybeUnwrap =
            if (builderIsFallible) {
                ".unwrap()"
            } else {
                ""
            }
        project.lib {
            unitTest(
                "query_serializer",
                """
                use test_model::{Choice, Top};

                let input = crate::test_input::OpInput::builder()
                    .top(
                        Top::builder()
                            .field("Hello")
                            .choice(Choice::Boolean(true))
                            .extra(45)
                            .recursive(
                                Top::builder()
                                    .field("World!")
                                    .choice(Choice::Boolean(true))
                                    .extra(55)
                                    .build()
                                    $maybeUnwrap
                            )
                            .build()
                            $maybeUnwrap
                    )
                    .boolean(true)
                    .blob(aws_smithy_types::Blob::new(&b"test"[..]))
                    .build()
                    .unwrap();
                let serialized = ${format(operationGenerator!!)}(&input).unwrap();
                let output = std::str::from_utf8(serialized.bytes().unwrap()).unwrap();
                assert_eq!(
                    output,
                    "\
                    Action=Op\
                    &Version=test\
                    &some_bool=true\
                    &top.choice.choice=true\
                    &top.field=Hello\
                    &top.extra=45\
                    &top.rec.item.1.choice.choice=true\
                    &top.rec.item.1.field=World%21\
                    &top.rec.item.1.extra=55\
                    &blob=dGVzdA%3D%3D"
                );
                """,
            )
        }
        model.lookup<StructureShape>("test#Top").also { top ->
            top.renderWithModelBuilder(model, symbolProvider, project)
            project.moduleFor(top) {
                UnionGenerator(
                    model,
                    symbolProvider,
                    this,
                    model.lookup("test#Choice"),
                    renderUnknownVariant = generateUnknownVariant,
                ).render()
                val enum = model.lookup<StringShape>("test#FooEnum")
                EnumGenerator(model, symbolProvider, enum, TestEnumType, emptyList()).render(this)
            }
        }

        model.lookup<OperationShape>("test#Op").inputShape(model).also { input ->
            input.renderWithModelBuilder(model, symbolProvider, project)
        }
        project.compileAndTest()
    }

    @Test
    fun `union with unit struct demonstrates query serialization bug`() {
        val model =
            """
            namespace test

            union TestUnion {
                unitMember: Unit,
                dataMember: String
            }

            structure Unit {}
            """.asSmithyModel()

        val project = TestWorkspace.testProject(testSymbolProvider(model))
        val codegenContext = testCodegenContext(model)

        // Render the Unit structure
        model.lookup<StructureShape>("test#Unit").also { unit ->
            unit.renderWithModelBuilder(model, codegenContext.symbolProvider, project)
        }

        // Render the Union
        project.moduleFor(model.lookup<UnionShape>("test#TestUnion")) {
            UnionGenerator(model, codegenContext.symbolProvider, this, model.lookup("test#TestUnion")).render()
        }
        project.compileAndTest()
    }
}
