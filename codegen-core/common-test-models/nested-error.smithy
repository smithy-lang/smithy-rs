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
        ErrorWithCompositeShape,
        ErrorWithDeepCompositeShape,
        ComposedSensitiveError
    ]
}

@error("client")
structure ErrorWithCompositeShape {
    message: ErrorMessage
}

@error("client")
structure SimpleError {
    message: String
}

structure ErrorMessage {
    @required
    statusCode: String
    @required
    errorMessage: String
    requestId: String
    @required
    toolName: String
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
