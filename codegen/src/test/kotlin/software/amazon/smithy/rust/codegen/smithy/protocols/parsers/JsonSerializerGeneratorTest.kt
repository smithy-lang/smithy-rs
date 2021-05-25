package software.amazon.smithy.rust.codegen.smithy.protocols.parsers

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
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

class JsonSerializerGeneratorTest {
    private val baseModel = """
        namespace test
        use aws.protocols#restJson1

        union Choice {
            map: MyMap,
            list: SomeList,
            s: String,
            enum: FooEnum,
            date: Timestamp,
            number: Double,
            top: Top,
            blob: Blob,
            document: Document,
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
            @jsonName("rec")
            recursive: TopList
        }

        list TopList {
            member: Top
        }

        structure OpInput {
            @httpHeader("x-test")
            someHeader: String,
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
        val model = RecursiveShapeBoxer.transform(
            OperationNormalizer(baseModel).transformModel(
                OperationNormalizer.NoBody,
                OperationNormalizer.NoBody
            )
        )
        val symbolProvider = testSymbolProvider(model)
        val parserGenerator = JsonSerializerGenerator(testProtocolConfig(model))
        val payloadGenerator = parserGenerator.payloadSerializer(model.lookup("test#OpInput\$payload"))
        val operationGenerator = parserGenerator.operationSerializer(model.lookup("test#Op"))
        val documentGenerator = parserGenerator.documentSerializer()

        val project = TestWorkspace.testProject(testSymbolProvider(model))
        project.lib { writer ->
            writer.unitTest(
                """
                use model::Top;

                // Generate the operation/document serializers even if they're not directly tested
                // ${writer.format(operationGenerator!!)}
                // ${writer.format(documentGenerator)}

                let inp = crate::input::OpInput::builder().payload(
                    Top::builder()
                        .field("hello!")
                        .extra(45)
                        .recursive(Top::builder().extra(55).build())
                        .build()
                ).build().unwrap();
                let serialized = ${writer.format(payloadGenerator)}(&inp.payload.unwrap()).unwrap();
                let output = std::str::from_utf8(serialized.bytes().unwrap()).unwrap();
                assert_eq!(output, r#"{"field":"hello!","extra":45,"rec":[{"extra":55}]}"#);
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
        println("file:///${project.baseDir}/src/json_ser.rs")
        println("file:///${project.baseDir}/src/lib.rs")
        println("file:///${project.baseDir}/src/model.rs")
        println("file:///${project.baseDir}/src/operation_ser.rs")
        project.compileAndTest()
    }
}
