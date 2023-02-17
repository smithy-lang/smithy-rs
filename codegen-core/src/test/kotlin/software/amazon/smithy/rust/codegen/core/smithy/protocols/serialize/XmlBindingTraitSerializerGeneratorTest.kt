/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.protocols.serialize

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.TestEnumType
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
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
    private val baseModel = """
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

    @Test
    fun `generates valid serializers`() {
        val model = RecursiveShapeBoxer().transform(OperationNormalizer.transform(baseModel))
        val codegenContext = testCodegenContext(model)
        val symbolProvider = codegenContext.symbolProvider
        val parserGenerator = XmlBindingTraitSerializerGenerator(
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
                let inp = crate::test_input::OpInput::builder().payload(
                   Top::builder()
                       .field("hello!")
                       .extra(45)
                       .recursive(Top::builder().extra(55).build())
                       .build()
                ).build().unwrap();
                let serialized = ${format(operationSerializer)}(&inp.payload.unwrap()).unwrap();
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
                EnumGenerator(model, symbolProvider, enum, TestEnumType).render(this)
            }
        }
        model.lookup<OperationShape>("test#Op").inputShape(model).also { input ->
            input.renderWithModelBuilder(model, symbolProvider, project)
        }
        project.compileAndTest()
    }
}
