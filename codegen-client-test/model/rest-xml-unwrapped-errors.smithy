// This file defines test cases that test error serialization.

$version: "1.0"

namespace aws.protocoltests.restxmlunwrapped

use aws.protocols#restXml
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests
use aws.api#service


/// A REST XML service that sends XML requests and responses.
@service(sdkId: "Rest XML Protocol")
@restXml(noErrorWrapping: true)
service RestXmlExtrasUnwrappedErrors {
    version: "2019-12-16",
    operations: [GreetingWithUnwrappedErrors]
}

/// This operation has three possible return values:
///
/// 1. A successful response in the form of GreetingWithErrorsOutput
/// 2. An InvalidGreeting error.
/// 3. A BadRequest error.
///
/// Implementations must be able to successfully take a response and
/// properly (de)serialize successful and error responses based on the
/// the presence of the
@idempotent
@http(uri: "/GreetingWithErrors", method: "PUT")
operation GreetingWithUnwrappedErrors {
    output: GreetingWithErrorsOutput,
    errors: [InvalidGreetingUnwrapped, ComplexErrorUnwrapped]
}

apply GreetingWithUnwrappedErrors @httpResponseTests([
    {
        id: "GreetingWithErrorsUnwrapped",
        documentation: "Ensures that operations with errors successfully know how to deserialize the successful response",
        protocol: restXml,
        code: 200,
        body: "",
        headers: {
            "X-Greeting": "Hello"
        },
        params: {
            greeting: "Hello"
        }
    }
])

structure GreetingWithErrorsOutput {
    @httpHeader("X-Greeting")
    greeting: String,
}

/// This error is thrown when an invalid greeting value is provided.
@error("client")
@httpError(400)
structure InvalidGreetingUnwrapped {
    Message: String,
}

apply InvalidGreetingUnwrapped @httpResponseTests([
    {
        id: "InvalidGreetingErrorUnwrapped",
        documentation: "Parses simple XML errors",
        protocol: restXml,
        params: {
            Message: "Hi"
        },
        code: 400,
        headers: {
            "Content-Type": "application/xml"
        },
        body: """
                 <Error>
                    <Type>Sender</Type>
                    <Code>InvalidGreetingUnwrapped</Code>
                    <Message>Hi</Message>
                    <AnotherSetting>setting</AnotherSetting>
                    <RequestId>foo-id</RequestId>
                 </Error>
              """,
        bodyMediaType: "application/xml",
    }
])

/// This error is thrown when a request is invalid.
@error("client")
@httpError(403)
structure ComplexErrorUnwrapped {
    // Errors support HTTP bindings!
    @httpHeader("X-Header")
    Header: String,

    TopLevel: String,

    Nested: ComplexNestedErrorData,
}

apply ComplexErrorUnwrapped @httpResponseTests([
    {
        id: "ComplexErrorUnwrapped",
        protocol: restXml,
        params: {
            Header: "Header",
            TopLevel: "Top level",
            Nested: {
                Foo: "bar"
            }
        },
        code: 400,
        headers: {
            "Content-Type": "application/xml",
            "X-Header": "Header",
        },
        body: """
                 <Error>
                    <Type>Sender</Type>
                    <Code>ComplexErrorUnwrapped</Code>
                    <Message>Hi</Message>
                    <TopLevel>Top level</TopLevel>
                    <Nested>
                        <Foo>bar</Foo>
                    </Nested>
                 <RequestId>foo-id</RequestId>
                 </Error>
              """,
        bodyMediaType: "application/xml",
    }
])

structure ComplexNestedErrorData {
    Foo: String,
}
