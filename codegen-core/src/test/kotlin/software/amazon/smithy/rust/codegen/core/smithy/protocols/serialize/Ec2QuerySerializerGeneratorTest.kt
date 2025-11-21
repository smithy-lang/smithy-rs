/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

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
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.core.testutil.testRustSettings
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.lookup

class Ec2QuerySerializerGeneratorTest {
    private val baseModel =
        """
        namespace test

        union Choice {
            bigInt: BigInteger,
            bigDec: BigDecimal,
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
    @CsvSource(
        "CLIENT",
        "CLIENT_CAREFUL",
        "CLIENT_ZERO_VALUE_V1",
        "CLIENT_ZERO_VALUE_V1_NO_INPUT",
        "SERVER",
    )
    fun `generates valid serializers`(nullabilityCheckMode: NullableIndex.CheckMode) {
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(baseModel))
        val settings = testRustSettings()
        val codegenContext = testCodegenContext(model, settings = settings, nullabilityCheckMode = nullabilityCheckMode)
        val symbolProvider = codegenContext.symbolProvider
        val parserGenerator = Ec2QuerySerializerGenerator(codegenContext)
        val operationGenerator = parserGenerator.operationInputSerializer(model.lookup("test#Op"))

        val project = TestWorkspace.testProject(testSymbolProvider(model))
        project.lib {
            unitTest(
                "ec2query_serializer",
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
                    &Some_bool=true\
                    &Top.Field=hello%21\
                    &Top.Extra=45\
                    &Top.Rec.1.Extra=55\
                    "
                );
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
        "CLIENT",
        "CLIENT_CAREFUL",
        "CLIENT_ZERO_VALUE_V1",
        "CLIENT_ZERO_VALUE_V1_NO_INPUT",
        "SERVER",
    )
    fun `generates valid serializers for required types`(nullabilityCheckMode: NullableIndex.CheckMode) {
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(baseModelWithRequiredTypes))
        val settings = testRustSettings()
        val codegenContext = testCodegenContext(model, settings = settings, nullabilityCheckMode = nullabilityCheckMode)
        val symbolProvider = codegenContext.symbolProvider
        val parserGenerator = Ec2QuerySerializerGenerator(codegenContext)
        val operationGenerator = parserGenerator.operationInputSerializer(model.lookup("test#Op"))

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
                "ec2query_serializer",
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
                    &Some_bool=true\
                    &Top.Choice.Choice=true\
                    &Top.Field=Hello\
                    &Top.Extra=45\
                    &Top.Rec.1.Choice.Choice=true\
                    &Top.Rec.1.Field=World%21\
                    &Top.Rec.1.Extra=55\
                    &Blob=dGVzdA%3D%3D"
                );
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

    @ParameterizedTest
    @CsvSource("true", "false")
    fun `serializes big numbers correctly`(generateUnknownVariant: Boolean) {
        val model =
            """
            namespace test
            use aws.protocols#ec2Query

            @ec2Query
            @xmlNamespace(uri: "https://example.com")
            service TestService {
                version: "test",
                operations: [Op]
            }

            structure OpInput {
                bigInt: BigInteger,
                bigDec: BigDecimal,
            }

            @http(uri: "/", method: "POST")
            operation Op {
                input: OpInput,
            }
            """.asSmithyModel()

        val codegenContext = testCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider
        val serializerGenerator = Ec2QuerySerializerGenerator(codegenContext)
        val operationGenerator = serializerGenerator.operationInputSerializer(model.lookup("test#Op"))

        val project = TestWorkspace.testProject(symbolProvider)
        project.lib {
            unitTest(
                "big_number_serializer",
                """
                use aws_smithy_types::{BigInteger, BigDecimal};

                let input = crate::test_model::OpInput::builder()
                    .big_int(BigInteger::from("12345678901234567890".to_string()))
                    .big_dec(BigDecimal::from("123.456".to_string()))
                    .build();
                let serialized = ${format(operationGenerator!!)}(&input).unwrap();
                let output = std::str::from_utf8(serialized.bytes().unwrap()).unwrap();
                assert_eq!(
                    output,
                    "Action=Op&Version=test&BigInt=12345678901234567890&BigDec=123.456"
                );
                """,
            )
        }

        model.lookup<OperationShape>("test#Op").inputShape(model).also { input ->
            input.renderWithModelBuilder(model, symbolProvider, project)
        }
        project.compileAndTest()
    }
}
