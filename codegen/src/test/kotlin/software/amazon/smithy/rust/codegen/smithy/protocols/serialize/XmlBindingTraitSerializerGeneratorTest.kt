/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.serialize

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpTraitHttpBindingResolver
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.testutil.testProtocolConfig
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.lookup

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

            blob: Blob
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
        val model = RecursiveShapeBoxer.transform(OperationNormalizer(baseModel).transformModel())
        val symbolProvider = testSymbolProvider(model)
        val parserGenerator = XmlBindingTraitSerializerGenerator(
            testProtocolConfig(model),
            HttpTraitHttpBindingResolver(model, "application/xml", null)
        )
        val operationParser = parserGenerator.payloadSerializer(model.lookup("test#OpInput\$payload"))

        val project = TestWorkspace.testProject(testSymbolProvider(model))
        project.lib { writer ->
            writer.unitTest(
                """
                 use model::Top;
                let inp = crate::input::OpInput::builder().payload(
                    Top::builder()
                        .field("hello!")
                        .extra(45)
                        .recursive(Top::builder().extra(55).build())
                        .build()
                ).build().unwrap();
                let serialized = ${writer.format(operationParser)}(&inp.payload.unwrap()).unwrap();
                let output = std::str::from_utf8(serialized.bytes().unwrap()).unwrap();
                assert_eq!(output, "<Top extra=\"45\"><field>hello!</field><recursive extra=\"55\"></recursive></Top>");
            """
            )
        }
        project.withModule(RustModule.default("model", public = true)) {
            model.lookup<StructureShape>("test#Top").renderWithModelBuilder(model, symbolProvider, it)
            UnionGenerator(model, symbolProvider, it, model.lookup("test#Choice")).render()
            val enum = model.lookup<StringShape>("test#FooEnum")
            EnumGenerator(model, symbolProvider, it, enum, enum.expectTrait()).render()
        }

        project.withModule(RustModule.default("input", public = true)) {
            model.lookup<OperationShape>("test#Op").inputShape(model).renderWithModelBuilder(model, symbolProvider, it)
        }
        println("file:///${project.baseDir}/src/xml_ser.rs")
        project.compileAndTest()
    }
}
