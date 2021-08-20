/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols.parse

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
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.codegen.util.outputShape

class JsonParserGeneratorTest {
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
            @required
            choice: Choice,
            field: String,
            extra: Integer,
            @jsonName("rec")
            recursive: TopList
        }

        list TopList {
            member: Top
        }

        structure OpOutput {
            @httpHeader("x-test")
            someHeader: String,

            top: Top
        }

        @error("client")
        structure Error {
            message: String,
            reason: String
        }

        @http(uri: "/top", method: "POST")
        operation Op {
            output: OpOutput,
            errors: [Error]
        }
    """.asSmithyModel()

    @Test
    fun `generates valid deserializers`() {
        val model = RecursiveShapeBoxer.transform(OperationNormalizer.transform(baseModel))
        val symbolProvider = testSymbolProvider(model)
        val parserGenerator = JsonParserGenerator(
            testProtocolConfig(model),
            HttpTraitHttpBindingResolver(model, "application/json", "application/json")
        )
        val operationGenerator = parserGenerator.operationParser(model.lookup("test#Op"))
        val documentGenerator = parserGenerator.documentParser(model.lookup("test#Op"))
        val payloadGenerator = parserGenerator.payloadParser(model.lookup("test#OpOutput\$top"))
        val errorParser = parserGenerator.errorParser(model.lookup("test#Error"))

        val project = TestWorkspace.testProject(testSymbolProvider(model))
        project.lib { writer ->
            writer.unitTest(
                """
                use model::Choice;

                // Generate the document serializer even though it's not tested directly
                // ${writer.format(documentGenerator)}
                // ${writer.format(payloadGenerator)}

                let json = br#"
                    { "top":
                        { "extra": 45,
                          "field": "something",
                          "choice":
                              { "int": 5 }}}
                "#;

                let output = ${writer.format(operationGenerator!!)}(json, output::op_output::Builder::default()).unwrap().build();
                let top = output.top.expect("top");
                assert_eq!(Some(45), top.extra);
                assert_eq!(Some("something".to_string()), top.field);
                assert_eq!(Some(Choice::Int(5)), top.choice);
                let output = ${writer.format(operationGenerator!!)}(b"", output::op_output::Builder::default()).unwrap().build();
                assert_eq!(output.top, None);


                // empty error
                let error_output = ${writer.format(errorParser!!)}(b"", error::error::Builder::default()).unwrap().build();
                assert_eq!(error_output.message, None);

                // error with message
                let error_output = ${writer.format(errorParser!!)}(br#"{"message": "hello"}"#, error::error::Builder::default()).unwrap().build();
                assert_eq!(error_output.message.expect("message should be set"), "hello");
                """
            )
        }
        project.withModule(RustModule.default("model", public = true)) {
            model.lookup<StructureShape>("test#Top").renderWithModelBuilder(model, symbolProvider, it)
            UnionGenerator(model, symbolProvider, it, model.lookup("test#Choice")).render()
            val enum = model.lookup<StringShape>("test#FooEnum")
            EnumGenerator(model, symbolProvider, it, enum, enum.expectTrait()).render()
        }

        project.withModule(RustModule.default("output", public = true)) {
            model.lookup<OperationShape>("test#Op").outputShape(model).renderWithModelBuilder(model, symbolProvider, it)
        }
        project.withModule(RustModule.default("error", public = true)) {
            model.lookup<StructureShape>("test#Error").renderWithModelBuilder(model, symbolProvider, it)
        }
        println("file:///${project.baseDir}/src/json_deser.rs")
        println("file:///${project.baseDir}/src/lib.rs")
        println("file:///${project.baseDir}/src/model.rs")
        println("file:///${project.baseDir}/src/output.rs")
        project.compileAndTest()
    }
}
