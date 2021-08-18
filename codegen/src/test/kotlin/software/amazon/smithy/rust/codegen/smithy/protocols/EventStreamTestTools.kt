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
        val protocolBuilder: (ProtocolConfig) -> Protocol,
    )

    class ModelArgumentsProvider : ArgumentsProvider {
        override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
            Stream.of(
                Arguments.of(TestCase("aws.protocols#restJson1", restJson1()) { RestJson(it) }),
                Arguments.of(TestCase("aws.protocols#restXml", restXml()) { RestXml(it) }),
                Arguments.of(TestCase("aws.protocols#awsJson1_1", awsJson11()) { AwsJson(it, AwsJsonVersion.Json11) }),
                Arguments.of(TestCase("aws.protocols#awsQuery", awsQuery()) { AwsQueryProtocol(it) }),
                Arguments.of(TestCase("aws.protocols#ec2Query", ec2Query()) { Ec2QueryProtocol(it) }),
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
        val model = OperationNormalizer(model).transformModel()
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
