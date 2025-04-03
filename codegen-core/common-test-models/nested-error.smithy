$version: "2"

namespace sample

use smithy.framework#ValidationException
use aws.protocols#restJson1

@restJson1
service SampleService {
    operations: [SampleOperation]
}

@http(uri: "/anOperation", method: "POST")
operation SampleOperation {
    output:= {}
    input:= {}
    errors: [
        SimpleError,
        ErrorInInput,
        ErrorWithDeepCompositeShape,
        ComposedSensitiveError,
    ]
}

@error("client")
structure SimpleError {
    message: String
}

@error("client")
structure ErrorInInput {
    message: ErrorMessage
}

structure ErrorMessage {
    @required
    statusCode: Integer
    @required
    errorMessage: String
    requestId: String
    @required
    isRetryable: Boolean
}

structure WrappedErrorMessage {
    someValue: Integer
    contained: ErrorMessage
}

@error("client")
structure ErrorWithDeepCompositeShape {
    message: WrappedErrorMessage
}

@sensitive
structure SensitiveMessage {
    nothing: String
    should: String
    bePrinted: String
}

@error("server")
structure ComposedSensitiveError {
    message: SensitiveMessage
}

@error("server")
structure ErrorWithNestedError {
    message: ErrorWithDeepCompositeShape
}
