/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

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
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.ErrorsModule
import software.amazon.smithy.rust.codegen.core.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.CombinedErrorGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ServerCombinedErrorGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsQueryProtocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Ec2QueryProtocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestXml
import software.amazon.smithy.rust.codegen.core.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.core.testutil.TestWriterDelegator
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.renderWithModelBuilder
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestSymbolProvider
import java.util.stream.Stream
import kotlin.streams.toList

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
    structure TestStreamInputOutput { @httpPayload @required value: TestStream }
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
    private fun restJson1(): Model = fillInBaseModel("restJson1").asSmithyModel()
    private fun restXml(): Model = fillInBaseModel("restXml").asSmithyModel()
    private fun awsJson11(): Model = fillInBaseModel("awsJson1_1").asSmithyModel()
    private fun awsQuery(): Model =
        fillInBaseModel("awsQuery", "@xmlNamespace(uri: \"https://example.com\")").asSmithyModel()
    private fun ec2Query(): Model =
        fillInBaseModel("ec2Query", "@xmlNamespace(uri: \"https://example.com\")").asSmithyModel()

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
        val target: CodegenTarget = CodegenTarget.CLIENT,
        val protocolBuilder: (CodegenContext) -> Protocol,
    ) {
        override fun toString(): String = protocolShapeId
    }

    private val testCases = listOf(
        //
        // restJson1
        //
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
        ) { RestJson(it) },

        //
        // restJson1, server mode
        //
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
        ) { RestJson(it) },

        //
        // awsJson1_1
        //
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
        ) { AwsJson(it, AwsJsonVersion.Json11) },

        //
        // restXml
        //
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
        ) { RestXml(it) },

        //
        // awsQuery
        //
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
        ) { AwsQueryProtocol(it) },

        //
        // ec2Query
        //
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
        ) { Ec2QueryProtocol(it) },
    )
    // TODO(https://github.com/awslabs/smithy-rs/issues/1442) Server tests
    //  should be run from the server subproject using the
    //  `serverTestSymbolProvider()`.
    // .flatMap { listOf(it, it.copy(target = CodegenTarget.SERVER)) }

    class UnmarshallTestCasesProvider : ArgumentsProvider {
        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
            testCases.map { Arguments.of(it) }.stream()
    }

    class MarshallTestCasesProvider : ArgumentsProvider {
        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
            // Don't include awsQuery or ec2Query for now since marshall support for them is unimplemented
            testCases
                .filter { testCase -> !testCase.protocolShapeId.contains("Query") }
                .map { Arguments.of(it) }.stream()
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
    fun generateTestProject(testCase: EventStreamTestModels.TestCase): TestEventStreamProject {
        val model = EventStreamNormalizer.transform(OperationNormalizer.transform(testCase.model))
        val serviceShape = model.expectShape(ShapeId.from("test#TestService")) as ServiceShape
        val operationShape = model.expectShape(ShapeId.from("test#TestStreamOp")) as OperationShape
        val unionShape = model.expectShape(ShapeId.from("test#TestStream")) as UnionShape

        val symbolProvider = when (testCase.target) {
            CodegenTarget.CLIENT -> testSymbolProvider(model)
            CodegenTarget.SERVER -> serverTestSymbolProvider(model)
        }
        val project = TestWorkspace.testProject(symbolProvider)
        val operationSymbol = symbolProvider.toSymbol(operationShape)
        project.withModule(ErrorsModule) {
            val errors = model.shapes()
                .filter { shape -> shape.isStructureShape && shape.hasTrait<ErrorTrait>() }
                .map { it.asStructureShape().get() }
                .toList()
            when (testCase.target) {
                CodegenTarget.CLIENT -> CombinedErrorGenerator(model, symbolProvider, operationSymbol, errors).render(this)
                CodegenTarget.SERVER -> ServerCombinedErrorGenerator(model, symbolProvider, operationSymbol, errors).render(this)
            }
            for (shape in model.shapes().filter { shape -> shape.isStructureShape && shape.hasTrait<ErrorTrait>() }) {
                StructureGenerator(model, symbolProvider, this, shape as StructureShape).render(testCase.target)
                val builderGen = BuilderGenerator(model, symbolProvider, shape)
                builderGen.render(this)
                implBlock(shape, symbolProvider) {
                    builderGen.renderConvenienceMethod(this)
                }
            }
        }
        project.withModule(ModelsModule) {
            val inputOutput = model.lookup<StructureShape>("test#TestStreamInputOutput")
            recursivelyGenerateModels(model, symbolProvider, inputOutput, this, testCase.target)
        }
        project.withModule(RustModule.Output) {
            operationShape.outputShape(model).renderWithModelBuilder(model, symbolProvider, this)
        }
        return TestEventStreamProject(model, serviceShape, operationShape, unionShape, symbolProvider, project)
    }

    private fun recursivelyGenerateModels(
        model: Model,
        symbolProvider: RustSymbolProvider,
        shape: Shape,
        writer: RustWriter,
        mode: CodegenTarget,
    ) {
        for (member in shape.members()) {
            val target = model.expectShape(member.target)
            if (target is StructureShape || target is UnionShape) {
                if (target is StructureShape) {
                    target.renderWithModelBuilder(model, symbolProvider, writer)
                } else if (target is UnionShape) {
                    UnionGenerator(model, symbolProvider, writer, target, renderUnknownVariant = mode.renderUnknownVariant()).render()
                }
                recursivelyGenerateModels(model, symbolProvider, target, writer, mode)
            }
        }
    }
}
