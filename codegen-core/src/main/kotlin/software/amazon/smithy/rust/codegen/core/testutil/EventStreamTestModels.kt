/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import software.amazon.smithy.model.Model
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestJson
import software.amazon.smithy.rust.codegen.core.smithy.protocols.RestXml

private fun fillInBaseModel(
    protocolName: String,
    extraServiceAnnotations: String = "",
): String =
    """
    namespace test

    use smithy.framework#ValidationException
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

    structure TestStreamInputOutput {
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
        val mediaType: String,
        val requestContentType: String,
        val responseContentType: String,
        val eventStreamMessageContentType: String,
        val validTestStruct: String,
        val validMessageWithNoHeaderPayloadTraits: String,
        val validTestUnion: String,
        val validSomeError: String,
        val validUnmodeledError: String,
        val protocolBuilder: (CodegenContext) -> Protocol,
    ) {
        override fun toString(): String = protocolShapeId
    }

    val TEST_CASES =
        listOf(
            //
            // restJson1
            //
            TestCase(
                protocolShapeId = "aws.protocols#restJson1",
                model = restJson1(),
                mediaType = "application/json",
                requestContentType = "application/vnd.amazon.eventstream",
                responseContentType = "application/json",
                eventStreamMessageContentType = "application/json",
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
                mediaType = "application/x-amz-json-1.1",
                requestContentType = "application/x-amz-json-1.1",
                responseContentType = "application/x-amz-json-1.1",
                eventStreamMessageContentType = "application/json",
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
