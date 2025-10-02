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
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.TestEnumType
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolContentTypes
import software.amazon.smithy.rust.codegen.core.smithy.protocols.restJsonFieldName
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.SerializerGeneratorTestUtils.UnionWithEmptyStructShapeIds
import software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize.SerializerGeneratorTestUtils.unionWithEmptyStructModel
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

class JsonSerializerGeneratorTest {
    private val baseModel =
        """
        namespace test
        use aws.protocols#restJson1

        union Choice {
            blob: Blob,
            boolean: Boolean,
            date: Timestamp,
            document: Document,
            enum: FooEnum,
            int: Integer,
            list: SomeList,
            listSparse: SomeSparseList,
            long: Long,
            map: MyMap,
            mapSparse: MySparseMap,
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

        @sparse
        map MySparseMap {
            key: String,
            value: Choice,
        }

        list SomeList {
            member: Choice
        }

        @sparse
        list SomeSparseList {
            member: Choice
        }

        structure Top {
            choice: Choice,
            field: String,
            extra: Long,
            @jsonName("rec")
            recursive: TopList
        }

        list TopList {
            member: Top
        }

        @input
        structure OpInput {
            @httpHeader("x-test")
            someHeader: String,

            top: Top
        }

        @http(uri: "/top", method: "POST")
        operation Op {
            input: OpInput,
        }
        """.asSmithyModel()

    @ParameterizedTest
    @CsvSource(
        "CLIENT",
        "CLIENT_CAREFUL",
        "CLIENT_ZERO_VALUE_V1",
        "CLIENT_ZERO_VALUE_V1_NO_INPUT",
        "SERVER",
    )
    fun `generates valid serializers`(nullabilityCheckMode: NullableIndex.CheckMode) {
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(baseModel))
        val codegenContext = testCodegenContext(model, nullabilityCheckMode = nullabilityCheckMode)
        val symbolProvider = codegenContext.symbolProvider
        val parserSerializer =
            JsonSerializerGenerator(
                codegenContext,
                HttpTraitHttpBindingResolver(model, ProtocolContentTypes.consistent("application/json")),
                ::restJsonFieldName,
            )
        val operationGenerator = parserSerializer.operationInputSerializer(model.lookup("test#Op"))
        val documentGenerator = parserSerializer.documentSerializer()

        val project = TestWorkspace.testProject(symbolProvider)
        project.lib {
            unitTest(
                "json_serializers",
                """
                use test_model::{Top, Choice};

                // Generate the document serializer even though it's not tested directly
                // ${format(documentGenerator)}

                let input = crate::test_input::OpInput::builder().top(
                    Top::builder()
                        .field("hello!")
                        .extra(45)
                        .recursive(Top::builder().extra(55).build())
                        .build()
                ).build().unwrap();
                let serialized = ${format(operationGenerator!!)}(&input).unwrap();
                let output = std::str::from_utf8(serialized.bytes().unwrap()).unwrap();
                assert_eq!(output, r#"{"top":{"field":"hello!","extra":45,"rec":[{"extra":55}]}}"#);

                let input = crate::test_input::OpInput::builder().top(
                    Top::builder()
                        .choice(Choice::Unknown)
                        .build()
                ).build().unwrap();
                ${format(operationGenerator)}(&input).expect_err("cannot serialize unknown variant");
                """,
            )
        }
        model.lookup<StructureShape>("test#Top").also { top ->
            top.renderWithModelBuilder(model, symbolProvider, project)
            project.moduleFor(top) {
                UnionGenerator(model, symbolProvider, this, model.lookup("test#Choice")).render()
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
            document: Document,
            enum: FooEnum,
            int: Integer,
            list: SomeList,
            listSparse: SomeSparseList,
            long: Long,
            map: MyMap,
            mapSparse: MySparseMap,
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

        @sparse
        map MySparseMap {
            key: String,
            value: Choice,
        }

        list SomeList {
            member: Choice
        }

        @sparse
        list SomeSparseList {
            member: Choice
        }

        structure Top {
            @required
            choice: Choice,
            @required
            field: String,
            @required
            extra: Long,
            @jsonName("rec")
            recursive: TopList
        }

        list TopList {
            member: Top
        }

        @input
        structure OpInput {
            @httpHeader("x-test")
            someHeader: String,

            @required
            boolean: Boolean,
            list: SomeList,
            map: MyMap,

            @required
            top: Top
        }

        @http(uri: "/top", method: "POST")
        operation Op {
            input: OpInput,
        }
        """.asSmithyModel()

    @ParameterizedTest
    @CsvSource(
        "CLIENT",
        "CLIENT_CAREFUL",
        "CLIENT_ZERO_VALUE_V1",
        "CLIENT_ZERO_VALUE_V1_NO_INPUT",
        "SERVER",
    )
    fun `generates valid serializers for required types`(nullabilityCheckMode: NullableIndex.CheckMode) {
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(baseModelWithRequiredTypes))
        val codegenContext = testCodegenContext(model, nullabilityCheckMode = nullabilityCheckMode)
        val symbolProvider = codegenContext.symbolProvider
        val parserSerializer =
            JsonSerializerGenerator(
                codegenContext,
                HttpTraitHttpBindingResolver(model, ProtocolContentTypes.consistent("application/json")),
                ::restJsonFieldName,
            )
        val operationGenerator = parserSerializer.operationInputSerializer(model.lookup("test#Op"))
        val documentGenerator = parserSerializer.documentSerializer()

        val project = TestWorkspace.testProject(testSymbolProvider(model))

        // Depending on the nullability check mode, the builder can be fallible or not. When it's fallible, we need to
        // add unwrap calls.
        val builderIsFallible =
            BuilderGenerator.hasFallibleBuilder(model.lookup<StructureShape>("test#Top"), symbolProvider)
        val maybeUnwrap =
            if (builderIsFallible) {
                ".unwrap()"
            } else {
                ""
            }
        project.lib {
            unitTest(
                "json_serializers",
                """
                use test_model::{Choice, Top};

                // Generate the document serializer even though it's not tested directly
                // ${format(documentGenerator)}

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
                    .build()
                    .unwrap();
                let serialized = ${format(operationGenerator!!)}(&input).unwrap();
                let output = std::str::from_utf8(serialized.bytes().unwrap()).unwrap();
                assert_eq!(output, r#"{"boolean":true,"top":{"choice":{"boolean":true},"field":"Hello","extra":45,"rec":[{"choice":{"boolean":true},"field":"World!","extra":55}]}}"#);

                let input = crate::test_input::OpInput::builder().top(
                    Top::builder()
                        .field("Hello")
                        .choice(Choice::Unknown)
                        .extra(45)
                        .build()
                        $maybeUnwrap
                ).boolean(false).build().unwrap();
                ${format(operationGenerator)}(&input).expect_err("cannot serialize unknown variant");
                """,
            )
        }
        model.lookup<StructureShape>("test#Top").also { top ->
            top.renderWithModelBuilder(model, symbolProvider, project)
            project.moduleFor(top) {
                UnionGenerator(model, symbolProvider, this, model.lookup("test#Choice")).render()
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
    fun `union with empty struct doesn't cause unused variable warning`() {
        // Regression test for https://github.com/smithy-lang/smithy-rs/issues/4308
        val model = OperationNormalizer.transform(unionWithEmptyStructModel)
        val codegenContext = testCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider
        val parserSerializer =
            JsonSerializerGenerator(
                codegenContext,
                HttpTraitHttpBindingResolver(model, ProtocolContentTypes.consistent("application/json")),
                ::restJsonFieldName,
            )
        val operationGenerator = parserSerializer.operationInputSerializer(model.lookup(UnionWithEmptyStructShapeIds.TEST_OPERATION))

        val project = TestWorkspace.testProject(symbolProvider)
        project.lib {
            unitTest(
                "union_with_empty_struct_serialization",
                """
                use test_model::{ObjectEncryptionFilter, Sses3Filter};

                let input = crate::test_input::TestOperationInput::builder()
                    .filter(ObjectEncryptionFilter::Sses3(Sses3Filter::builder().build()))
                    .build()
                    .unwrap();
                let _serialized = ${format(operationGenerator!!)}(&input).unwrap();

                let input = crate::test_input::TestOperationInput::builder()
                    .filter(ObjectEncryptionFilter::Data("test".to_string()))
                    .build()
                    .unwrap();
                let _serialized = ${format(operationGenerator)}(&input).unwrap();
                """,
            )
        }

        model.lookup<StructureShape>(UnionWithEmptyStructShapeIds.EMPTY_STRUCT).also { emptyStruct ->
            emptyStruct.renderWithModelBuilder(model, symbolProvider, project)
        }

        model.lookup<StructureShape>(UnionWithEmptyStructShapeIds.TEST_INPUT).also { testInput ->
            testInput.renderWithModelBuilder(model, symbolProvider, project)
            project.moduleFor(testInput) {
                UnionGenerator(model, symbolProvider, this, model.lookup(UnionWithEmptyStructShapeIds.TEST_UNION)).render()
            }
        }

        model.lookup<OperationShape>(UnionWithEmptyStructShapeIds.TEST_OPERATION).inputShape(model).also { input ->
            input.renderWithModelBuilder(model, symbolProvider, project)
        }

        project.compileAndTest()
    }
}
