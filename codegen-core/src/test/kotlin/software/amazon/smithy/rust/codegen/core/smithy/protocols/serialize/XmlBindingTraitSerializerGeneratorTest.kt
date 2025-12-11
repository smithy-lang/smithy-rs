/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.CsvSource
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.TestEnumType
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolContentTypes
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

internal class XmlBindingTraitSerializerGeneratorTest {
    private val baseModel =
        """
        namespace test
        use aws.protocols#restXml
        union Choice {
            @xmlFlattened
            @xmlName("Hi")
            flatMap: MyMap,

            deepMap: MyMap,

            @xmlFlattened
            flatList: SomeList,

            deepList: SomeList,

            s: String,

            enum: FooEnum,

            date: Timestamp,

            number: Double,

            top: Top,

            blob: Blob,

            unit: Unit,
        }

        @enum([{name: "FOO", value: "FOO"}])
        string FooEnum

        map MyMap {
            @xmlName("Name")
            key: String,

            @xmlName("Setting")
            value: Choice,
        }

        list SomeList {
            member: Choice
        }


        structure Top {
            choice: Choice,

            field: String,

            @xmlAttribute
            extra: Long,

            @xmlName("prefix:local")
            renamedWithPrefix: String,

            @xmlFlattened
            recursive: TopList
        }

        list TopList {
            member: Top
        }

        structure OpInput {
            @httpPayload
            payload: Top
        }

        @http(uri: "/top", method: "POST")
        operation Op {
            input: OpInput,
        }
        """.asSmithyModel()

    private val bigNumberModel =
        """
        namespace test
        use aws.protocols#restXml

        structure BigNumberData {
            bigInt: BigInteger,
            bigDec: BigDecimal,
        }

        structure BigNumberInput {
            @httpPayload
            payload: BigNumberData
        }

        @http(uri: "/bignumber", method: "POST")
        operation BigNumberOp {
            input: BigNumberInput,
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
        val parserGenerator =
            XmlBindingTraitSerializerGenerator(
                codegenContext,
                HttpTraitHttpBindingResolver(model, ProtocolContentTypes.consistent("application/xml")),
            )
        val operationSerializer = parserGenerator.payloadSerializer(model.lookup("test#OpInput\$payload"))

        val project = TestWorkspace.testProject(testSymbolProvider(model))
        project.lib {
            unitTest(
                "serialize_xml",
                """
                use test_model::Top;
                let input = crate::test_input::OpInput::builder().payload(
                   Top::builder()
                       .field("hello!")
                       .extra(45)
                       .recursive(Top::builder().extra(55).build())
                       .build()
                ).build().unwrap();
                let serialized = ${format(operationSerializer)}(&input.payload.unwrap()).unwrap();
                let output = std::str::from_utf8(&serialized).unwrap();
                assert_eq!(output, "<Top extra=\"45\"><field>hello!</field><recursive extra=\"55\"></recursive></Top>");
                """,
            )
            unitTest(
                "unknown_variants",
                """
                use test_model::{Top, Choice};
                let input = crate::test_input::OpInput::builder().payload(
                    Top::builder()
                        .choice(Choice::Unknown)
                        .build()
                ).build().unwrap();
                ${format(operationSerializer)}(&input.payload.unwrap()).expect_err("cannot serialize unknown variant");
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
        use aws.protocols#restXml
        union Choice {
            boolean: Boolean,
            @xmlFlattened
            @xmlName("Hi")
            flatMap: MyMap,
            deepMap: MyMap,
            @xmlFlattened
            flatList: SomeList,
            deepList: SomeList,
            s: String,
            enum: FooEnum,
            date: Timestamp,
            number: Double,
            top: Top,
            blob: Blob,
            unit: Unit,
        }

        @enum([{name: "FOO", value: "FOO"}])
        string FooEnum

        map MyMap {
            @xmlName("Name")
            key: String,
            @xmlName("Setting")
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
            @xmlAttribute
            extra: Long,
            @xmlName("prefix:local")
            renamedWithPrefix: String,
            @xmlName("rec")
            @xmlFlattened
            recursive: TopList
        }

        list TopList {
            member: Top
        }

        @input
        structure OpInput {
            @required
            @httpPayload
            payload: Top
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
        val parserGenerator =
            XmlBindingTraitSerializerGenerator(
                codegenContext,
                HttpTraitHttpBindingResolver(model, ProtocolContentTypes.consistent("application/xml")),
            )
        val operationSerializer = parserGenerator.payloadSerializer(model.lookup("test#OpInput\$payload"))

        val project = TestWorkspace.testProject(symbolProvider)

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
        val payloadIsOptional =
            model.lookup<MemberShape>("test#OpInput\$payload").let {
                symbolProvider.toSymbol(it).isOptional()
            }
        val maybeUnwrapPayload =
            if (payloadIsOptional) {
                ".unwrap()"
            } else {
                ""
            }
        project.lib {
            unitTest(
                "serialize_xml",
                """
                use test_model::{Choice, Top};

                let input = crate::test_input::OpInput::builder()
                    .payload(
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
                    .build()
                    .unwrap();
                let serialized = ${format(operationSerializer)}(&input.payload$maybeUnwrapPayload).unwrap();
                let output = std::str::from_utf8(&serialized).unwrap();
                assert_eq!(output, "<Top extra=\"45\"><choice><boolean>true</boolean></choice><field>Hello</field><rec extra=\"55\"><choice><boolean>true</boolean></choice><field>World!</field></rec></Top>");
                """,
            )
            unitTest(
                "unknown_variants",
                """
                use test_model::{Choice, Top};
                let input = crate::test_input::OpInput::builder().payload(
                    Top::builder()
                        .field("Hello")
                        .choice(Choice::Unknown)
                        .extra(45)
                        .build()
                        $maybeUnwrap
                ).build().unwrap();
                ${format(operationSerializer)}(&input.payload$maybeUnwrapPayload).expect_err("cannot serialize unknown variant");
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

    @org.junit.jupiter.api.Test
    fun `serializes BigInteger and BigDecimal to XML`() {
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(bigNumberModel))
        val codegenContext = testCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider
        val serializerGenerator =
            XmlBindingTraitSerializerGenerator(
                codegenContext,
                HttpTraitHttpBindingResolver(model, ProtocolContentTypes.consistent("application/xml")),
            )
        val operationSerializer = serializerGenerator.payloadSerializer(model.lookup("test#BigNumberInput\$payload"))

        val project = TestWorkspace.testProject(testSymbolProvider(model))
        project.lib {
            unitTest(
                "serialize_big_numbers",
                """
                use aws_smithy_types::{BigInteger, BigDecimal};
                let input = crate::test_input::BigNumberOpInput::builder().payload(
                    crate::test_model::BigNumberData::builder()
                        .big_int("12345678901234567890".parse().unwrap())
                        .big_dec("3.141592653589793238".parse().unwrap())
                        .build()
                ).build().unwrap();
                let serialized = ${format(operationSerializer)}(&input.payload.unwrap()).unwrap();
                let output = std::str::from_utf8(&serialized).unwrap();
                assert!(output.contains("<bigInt>12345678901234567890</bigInt>"));
                assert!(output.contains("<bigDec>3.141592653589793238</bigDec>"));
                """,
            )
        }

        model.lookup<StructureShape>("test#BigNumberData").also { struct ->
            struct.renderWithModelBuilder(model, symbolProvider, project)
        }
        model.lookup<OperationShape>("test#BigNumberOp").inputShape(model).also { input ->
            input.renderWithModelBuilder(model, symbolProvider, project)
        }
        project.compileAndTest()
    }
}
