/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
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
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolContentTypes
import software.amazon.smithy.rust.codegen.smithy.protocols.restJsonFieldName
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.testutil.testCodegenContext
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.expectTrait
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.lookup

class JsonSerializerGeneratorTest {
    private val baseModel = """
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

    @Test
    fun `generates valid serializers`() {
        val model = RecursiveShapeBoxer.transform(OperationNormalizer.transform(baseModel))
        val symbolProvider = testSymbolProvider(model)
        val parserSerializer = JsonSerializerGenerator(
            testCodegenContext(model),
            HttpTraitHttpBindingResolver(model, ProtocolContentTypes.consistent("application/json")),
            ::restJsonFieldName
        )
        val operationGenerator = parserSerializer.operationInputSerializer(model.lookup("test#Op"))
        val documentGenerator = parserSerializer.documentSerializer()

        val project = TestWorkspace.testProject(testSymbolProvider(model))
        project.lib { writer ->
            writer.unitTest(
                "json_serializers",
                """
                use model::{Top, Choice};

                // Generate the document serializer even though it's not tested directly
                // ${writer.format(documentGenerator)}

                let input = crate::input::OpInput::builder().top(
                    Top::builder()
                        .field("hello!")
                        .extra(45)
                        .recursive(Top::builder().extra(55).build())
                        .build()
                ).build().unwrap();
                let serialized = ${writer.format(operationGenerator!!)}(&input).unwrap();
                let output = std::str::from_utf8(serialized.bytes().unwrap()).unwrap();
                assert_eq!(output, r#"{"top":{"field":"hello!","extra":45,"rec":[{"extra":55}]}}"#);

                let input = crate::input::OpInput::builder().top(
                    Top::builder()
                        .choice(Choice::Unknown)
                        .build()
                ).build().unwrap();
                let serialized = ${writer.format(operationGenerator)}(&input).expect_err("cannot serialize unknown variant");
                """
            )
        }
        project.withModule(RustModule.public("model")) {
            model.lookup<StructureShape>("test#Top").renderWithModelBuilder(model, symbolProvider, it)
            UnionGenerator(model, symbolProvider, it, model.lookup("test#Choice")).render()
            val enum = model.lookup<StringShape>("test#FooEnum")
            EnumGenerator(model, symbolProvider, it, enum, enum.expectTrait()).render()
        }

        project.withModule(RustModule.public("input")) {
            model.lookup<OperationShape>("test#Op").inputShape(model).renderWithModelBuilder(model, symbolProvider, it)
        }
        project.compileAndTest()
    }
}
