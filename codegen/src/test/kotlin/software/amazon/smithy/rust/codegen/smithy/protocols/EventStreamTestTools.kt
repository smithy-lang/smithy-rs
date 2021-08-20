/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.error.CombinedErrorGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.TestWriterDelegator
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.codegen.util.outputShape
import java.util.stream.Stream

private fun fillInBaseModel(
    protocolName: String,
    extraServiceAnnotations: String = "",
): String = """
    namespace test

    use aws.protocols#$protocolName

    union TestUnion {
        Foo: String,
        Bar: Integer,
    }
    structure TestStruct {
        someString: String,
        someInt: Integer,
    }

    @error("client")
    structure SomeError {
        Message: String,
    }

    structure MessageWithBlob { @eventPayload data: Blob }
    structure MessageWithString { @eventPayload data: String }
    structure MessageWithStruct { @eventPayload someStruct: TestStruct }
    structure MessageWithUnion { @eventPayload someUnion: TestUnion }
    structure MessageWithHeaders {
        @eventHeader blob: Blob,
        @eventHeader boolean: Boolean,
        @eventHeader byte: Byte,
        @eventHeader int: Integer,
        @eventHeader long: Long,
        @eventHeader short: Short,
        @eventHeader string: String,
        @eventHeader timestamp: Timestamp,
    }
    structure MessageWithHeaderAndPayload {
        @eventHeader header: String,
        @eventPayload payload: Blob,
    }
    structure MessageWithNoHeaderPayloadTraits {
        someInt: Integer,
        someString: String,
    }

    @streaming
    union TestStream {
        MessageWithBlob: MessageWithBlob,
        MessageWithString: MessageWithString,
        MessageWithStruct: MessageWithStruct,
        MessageWithUnion: MessageWithUnion,
        MessageWithHeaders: MessageWithHeaders,
        MessageWithHeaderAndPayload: MessageWithHeaderAndPayload,
        MessageWithNoHeaderPayloadTraits: MessageWithNoHeaderPayloadTraits,
        SomeError: SomeError,
    }
    structure TestStreamInputOutput { @required value: TestStream }
    operation TestStreamOp {
        input: TestStreamInputOutput,
        output: TestStreamInputOutput,
        errors: [SomeError],
    }
    $extraServiceAnnotations
    @$protocolName
    service TestService { version: "123", operations: [TestStreamOp] }
"""

object EventStreamTestModels {
    fun restJson1(): Model = fillInBaseModel("restJson1").asSmithyModel()
    fun restXml(): Model = fillInBaseModel("restXml").asSmithyModel()
    fun awsJson11(): Model = fillInBaseModel("awsJson1_1").asSmithyModel()
    fun awsQuery(): Model = fillInBaseModel("awsQuery", "@xmlNamespace(uri: \"https://example.com\")").asSmithyModel()
    fun ec2Query(): Model = fillInBaseModel("ec2Query", "@xmlNamespace(uri: \"https://example.com\")").asSmithyModel()

    data class TestCase(
        val protocolShapeId: String,
        val model: Model,
        val requestContentType: String,
        val responseContentType: String,
        val validTestStruct: String,
        val validMessageWithNoHeaderPayloadTraits: String,
        val validTestUnion: String,
        val validSomeError: String,
        val validUnmodeledError: String,
        val protocolBuilder: (ProtocolConfig) -> Protocol,
    ) {
        override fun toString(): String = protocolShapeId
    }

    class ModelArgumentsProvider : ArgumentsProvider {
        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
            Stream.of(
                Arguments.of(
                    TestCase(
                        protocolShapeId = "aws.protocols#restJson1",
                        model = restJson1(),
                        requestContentType = "application/json",
                        responseContentType = "application/json",
                        validTestStruct = """{"someString":"hello","someInt":5}""",
                        validMessageWithNoHeaderPayloadTraits = """{"someString":"hello","someInt":5}""",
                        validTestUnion = """{"Foo":"hello"}""",
                        validSomeError = """{"Message":"some error"}""",
                        validUnmodeledError = """{"Message":"unmodeled error"}""",
                    ) { RestJson(it) }
                ),
                Arguments.of(
                    TestCase(
                        protocolShapeId = "aws.protocols#awsJson1_1",
                        model = awsJson11(),
                        requestContentType = "application/x-amz-json-1.1",
                        responseContentType = "application/x-amz-json-1.1",
                        validTestStruct = """{"someString":"hello","someInt":5}""",
                        validMessageWithNoHeaderPayloadTraits = """{"someString":"hello","someInt":5}""",
                        validTestUnion = """{"Foo":"hello"}""",
                        validSomeError = """{"Message":"some error"}""",
                        validUnmodeledError = """{"Message":"unmodeled error"}""",
                    ) { AwsJson(it, AwsJsonVersion.Json11) }
                ),
                Arguments.of(
                    TestCase(
                        protocolShapeId = "aws.protocols#restXml",
                        model = restXml(),
                        requestContentType = "application/xml",
                        responseContentType = "application/xml",
                        validTestStruct = """
                            <TestStruct>
                                <someString>hello</someString>
                                <someInt>5</someInt>
                            </TestStruct>
                        """.trimIndent(),
                        validMessageWithNoHeaderPayloadTraits = """
                            <MessageWithNoHeaderPayloadTraits>
                                <someString>hello</someString>
                                <someInt>5</someInt>
                            </MessageWithNoHeaderPayloadTraits>
                        """.trimIndent(),
                        validTestUnion = "<TestUnion><Foo>hello</Foo></TestUnion>",
                        validSomeError = """
                            <ErrorResponse>
                                <Error>
                                    <Type>SomeError</Type>
                                    <Code>SomeError</Code>
                                    <Message>some error</Message>
                                </Error>
                            </ErrorResponse>
                        """.trimIndent(),
                        validUnmodeledError = """
                            <ErrorResponse>
                                <Error>
                                    <Type>UnmodeledError</Type>
                                    <Code>UnmodeledError</Code>
                                    <Message>unmodeled error</Message>
                                </Error>
                            </ErrorResponse>
                        """.trimIndent(),
                    ) { RestXml(it) }
                ),
                Arguments.of(
                    TestCase(
                        protocolShapeId = "aws.protocols#awsQuery",
                        model = awsQuery(),
                        requestContentType = "application/x-www-form-urlencoded",
                        responseContentType = "text/xml",
                        validTestStruct = """
                            <TestStruct>
                                <someString>hello</someString>
                                <someInt>5</someInt>
                            </TestStruct>
                        """.trimIndent(),
                        validMessageWithNoHeaderPayloadTraits = """
                            <MessageWithNoHeaderPayloadTraits>
                                <someString>hello</someString>
                                <someInt>5</someInt>
                            </MessageWithNoHeaderPayloadTraits>
                        """.trimIndent(),
                        validTestUnion = "<TestUnion><Foo>hello</Foo></TestUnion>",
                        validSomeError = """
                            <ErrorResponse>
                                <Error>
                                    <Type>SomeError</Type>
                                    <Code>SomeError</Code>
                                    <Message>some error</Message>
                                </Error>
                            </ErrorResponse>
                        """.trimIndent(),
                        validUnmodeledError = """
                            <ErrorResponse>
                                <Error>
                                    <Type>UnmodeledError</Type>
                                    <Code>UnmodeledError</Code>
                                    <Message>unmodeled error</Message>
                                </Error>
                            </ErrorResponse>
                        """.trimIndent(),
                    ) { AwsQueryProtocol(it) }
                ),
                Arguments.of(
                    TestCase(
                        protocolShapeId = "aws.protocols#ec2Query",
                        model = ec2Query(),
                        requestContentType = "application/x-www-form-urlencoded",
                        responseContentType = "text/xml",
                        validTestStruct = """
                            <TestStruct>
                                <someString>hello</someString>
                                <someInt>5</someInt>
                            </TestStruct>
                        """.trimIndent(),
                        validMessageWithNoHeaderPayloadTraits = """
                            <MessageWithNoHeaderPayloadTraits>
                                <someString>hello</someString>
                                <someInt>5</someInt>
                            </MessageWithNoHeaderPayloadTraits>
                        """.trimIndent(),
                        validTestUnion = "<TestUnion><Foo>hello</Foo></TestUnion>",
                        validSomeError = """
                            <Response>
                                <Errors>
                                    <Error>
                                        <Type>SomeError</Type>
                                        <Code>SomeError</Code>
                                        <Message>some error</Message>
                                    </Error>
                                </Errors>
                            </Response>
                        """.trimIndent(),
                        validUnmodeledError = """
                            <Response>
                                <Errors>
                                    <Error>
                                        <Type>UnmodeledError</Type>
                                        <Code>UnmodeledError</Code>
                                        <Message>unmodeled error</Message>
                                    </Error>
                                </Errors>
                            </Response>
                        """.trimIndent(),
                    ) { Ec2QueryProtocol(it) }
                ),
            )
    }
}

data class TestEventStreamProject(
    val model: Model,
    val serviceShape: ServiceShape,
    val operationShape: OperationShape,
    val streamShape: UnionShape,
    val symbolProvider: RustSymbolProvider,
    val project: TestWriterDelegator,
)

object EventStreamTestTools {
    fun generateTestProject(model: Model): TestEventStreamProject {
        val model = EventStreamNormalizer.transform(OperationNormalizer.transform(model))
        val serviceShape = model.expectShape(ShapeId.from("test#TestService")) as ServiceShape
        val operationShape = model.expectShape(ShapeId.from("test#TestStreamOp")) as OperationShape
        val unionShape = model.expectShape(ShapeId.from("test#TestStream")) as UnionShape

        val symbolProvider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(symbolProvider)
        project.withModule(RustModule.default("error", public = true)) {
            CombinedErrorGenerator(model, symbolProvider, operationShape).render(it)
            for (shape in model.shapes().filter { shape -> shape.isStructureShape && shape.hasTrait<ErrorTrait>() }) {
                StructureGenerator(model, symbolProvider, it, shape as StructureShape).render()
                val builderGen = BuilderGenerator(model, symbolProvider, shape)
                builderGen.render(it)
                it.implBlock(shape, symbolProvider) {
                    builderGen.renderConvenienceMethod(this)
                }
            }
        }
        project.withModule(RustModule.default("model", public = true)) {
            val inputOutput = model.lookup<StructureShape>("test#TestStreamInputOutput")
            recursivelyGenerateModels(model, symbolProvider, inputOutput, it)
        }
        project.withModule(RustModule.default("output", public = true)) {
            operationShape.outputShape(model).renderWithModelBuilder(model, symbolProvider, it)
        }
        println("file:///${project.baseDir}/src/error.rs")
        println("file:///${project.baseDir}/src/event_stream.rs")
        println("file:///${project.baseDir}/src/event_stream_serde.rs")
        println("file:///${project.baseDir}/src/lib.rs")
        println("file:///${project.baseDir}/src/model.rs")
        return TestEventStreamProject(model, serviceShape, operationShape, unionShape, symbolProvider, project)
    }

    private fun recursivelyGenerateModels(
        model: Model,
        symbolProvider: RustSymbolProvider,
        shape: Shape,
        writer: RustWriter
    ) {
        for (member in shape.members()) {
            val target = model.expectShape(member.target)
            if (target is StructureShape || target is UnionShape) {
                if (target is StructureShape) {
                    target.renderWithModelBuilder(model, symbolProvider, writer)
                } else if (target is UnionShape) {
                    UnionGenerator(model, symbolProvider, writer, target).render()
                }
                recursivelyGenerateModels(model, symbolProvider, target, writer)
            }
        }
    }
}
