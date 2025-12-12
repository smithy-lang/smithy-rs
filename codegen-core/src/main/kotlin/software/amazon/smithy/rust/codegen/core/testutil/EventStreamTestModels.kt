/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import com.fasterxml.jackson.databind.ObjectMapper
import com.fasterxml.jackson.dataformat.cbor.CBORFactory
import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestXml
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RpcV2Cbor
import java.util.Base64

private fun fillInBaseModel(
    namespacedProtocolName: String,
    extraServiceAnnotations: String = "",
    nonEventStreamMembers: String = "",
    extraEventHeaderMembers: String = "",
    extraShapes: String = "",
): String =
    """
    ${"\$version: \"2\""}
    namespace test

    use smithy.framework#ValidationException
    use $namespacedProtocolName

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

    $extraShapes

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
        $extraEventHeaderMembers
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

    structure TestStreamInputOutput {
        $nonEventStreamMembers

        @required
        @httpPayload
        value: TestStream
    }

    @http(method: "POST", uri: "/test")
    operation TestStreamOp {
        input: TestStreamInputOutput,
        output: TestStreamInputOutput,
        errors: [SomeError, ValidationException],
    }

    $extraServiceAnnotations
    @${namespacedProtocolName.substringAfter("#")}
    service TestService { version: "123", operations: [TestStreamOp] }
    """

object EventStreamTestModels {
    private fun restJson1(): Model = fillInBaseModel("aws.protocols#restJson1").asSmithyModel()

    private fun restXml(): Model = fillInBaseModel("aws.protocols#restXml").asSmithyModel()

    private fun awsJson11(): Model = fillInBaseModel("aws.protocols#awsJson1_1").asSmithyModel()

    private fun rpcv2Cbor(): Model = fillInBaseModel("smithy.protocols#rpcv2Cbor").asSmithyModel()

    // Event streams are not supported in AWS query or EC2 query
    // See Important in https://smithy.io/2.0/aws/protocols/aws-query-protocol.html where it states
    // "This protocol does not support any kind of streaming requests or responses, including event streams."

    data class TestCase(
        val protocolShapeId: String,
        val model: Model,
        val mediaType: String,
        val accept: String,
        val requestContentType: String,
        val responseContentType: String,
        val eventStreamMessageContentType: String,
        val eventStreamInitialResponsePayload: String? = null,
        val validTestStruct: String,
        val validMessageWithNoHeaderPayloadTraits: String,
        val validTestUnion: String,
        val validSomeError: String,
        val validUnmodeledError: String,
        val protocolBuilder: (CodegenContext) -> Protocol,
    ) {
        override fun toString(): String = protocolShapeId

        fun withNonEventStreamMembers(nonEventStreamMembers: String): TestCase =
            this.copy(model = fillInBaseModel(this.protocolShapeId, "", nonEventStreamMembers).asSmithyModel())

        // Server doesn't support enum members in event streams, so this util allows Clients to test with those shapes
        fun withEnumMembers(): TestCase =
            this.copy(
                model =
                    fillInBaseModel(
                        this.protocolShapeId,
                        extraEventHeaderMembers = "@eventHeader enum: TheEnum,\n@eventHeader intEnum: FaceCard,",
                        extraShapes = """    enum TheEnum {
                            FOO
                            BAR
                            }

                            intEnum FaceCard {
                            JACK = 1
                            QUEEN = 2
                            KING = 3
                                            }""",
                    ).asSmithyModel(),
            )
    }

    private fun base64Encode(input: ByteArray): String {
        val encodedBytes = Base64.getEncoder().encode(input)
        return String(encodedBytes)
    }

    private fun createCborFromJson(jsonString: String): ByteArray {
        val jsonMapper = ObjectMapper()
        val cborMapper = ObjectMapper(CBORFactory())
        // Parse JSON string to a generic type.
        val jsonData = jsonMapper.readValue(jsonString, Any::class.java)
        // Convert the parsed data to CBOR.
        return cborMapper.writeValueAsBytes(jsonData)
    }

    fun base64EncodeJson(jsonString: String) = base64Encode(createCborFromJson(jsonString))

    private val restJsonTestCase =
        TestCase(
            protocolShapeId = "aws.protocols#restJson1",
            model = restJson1(),
            accept = "application/json",
            mediaType = "application/json",
            requestContentType = "application/vnd.amazon.eventstream",
            responseContentType = "application/json",
            eventStreamMessageContentType = "application/json",
            validTestStruct = """{"someString":"hello","someInt":5}""",
            validMessageWithNoHeaderPayloadTraits = """{"someString":"hello","someInt":5}""",
            validTestUnion = """{"Foo":"hello"}""",
            validSomeError = """{"Message":"some error"}""",
            validUnmodeledError = """{"Message":"unmodeled error"}""",
        ) { RestJson(it) }

    val TEST_CASES =
        listOf(
            //
            // restJson1
            //
            restJsonTestCase,
            //
            // rpcV2Cbor
            //
            restJsonTestCase.copy(
                protocolShapeId = "smithy.protocols#rpcv2Cbor",
                model = rpcv2Cbor(),
                // application/cbor is appended for backward compatibility with servers that only handle application/cbor
                // https://github.com/smithy-lang/smithy-rs/pull/4427#issuecomment-3602558313
                accept = "application/vnd.amazon.eventstream, application/cbor",
                mediaType = "application/cbor",
                requestContentType = "application/vnd.amazon.eventstream",
                responseContentType = "application/cbor",
                eventStreamMessageContentType = "application/cbor",
                validTestStruct = base64EncodeJson(restJsonTestCase.validTestStruct),
                validMessageWithNoHeaderPayloadTraits = base64EncodeJson(restJsonTestCase.validMessageWithNoHeaderPayloadTraits),
                validTestUnion = base64EncodeJson(restJsonTestCase.validTestUnion),
                validSomeError = base64EncodeJson(restJsonTestCase.validSomeError),
                validUnmodeledError = base64EncodeJson(restJsonTestCase.validUnmodeledError),
                protocolBuilder = { RpcV2Cbor(it) },
            ),
            //
            // awsJson1_1
            //
            restJsonTestCase.copy(
                protocolShapeId = "aws.protocols#awsJson1_1",
                model = awsJson11(),
                mediaType = "application/x-amz-json-1.1",
                requestContentType = "application/x-amz-json-1.1",
                responseContentType = "application/x-amz-json-1.1",
                eventStreamMessageContentType = "application/json",
            ) { AwsJson(it, AwsJsonVersion.Json11) },
            //
            // restXml
            //
            TestCase(
                protocolShapeId = "aws.protocols#restXml",
                model = restXml(),
                accept = "application/xml",
                mediaType = "application/xml",
                requestContentType = "application/vnd.amazon.eventstream",
                responseContentType = "application/xml",
                eventStreamMessageContentType = "application/xml",
                validTestStruct =
                    """
                    <TestStruct>
                        <someString>hello</someString>
                        <someInt>5</someInt>
                    </TestStruct>
                    """.trimIndent(),
                validMessageWithNoHeaderPayloadTraits =
                    """
                    <MessageWithNoHeaderPayloadTraits>
                        <someString>hello</someString>
                        <someInt>5</someInt>
                    </MessageWithNoHeaderPayloadTraits>
                    """.trimIndent(),
                validTestUnion = "<TestUnion><Foo>hello</Foo></TestUnion>",
                validSomeError =
                    """
                    <ErrorResponse>
                        <Error>
                            <Type>SomeError</Type>
                            <Code>SomeError</Code>
                            <Message>some error</Message>
                        </Error>
                    </ErrorResponse>
                    """.trimIndent(),
                validUnmodeledError =
                    """
                    <ErrorResponse>
                        <Error>
                            <Type>UnmodeledError</Type>
                            <Code>UnmodeledError</Code>
                            <Message>unmodeled error</Message>
                        </Error>
                    </ErrorResponse>
                    """.trimIndent(),
            ) { RestXml(it) },
        )
}
