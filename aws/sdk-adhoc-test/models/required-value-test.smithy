$version: "1.0"

namespace com.amazonaws.requiredvalues

@restJson1
@title("Test Service")
@service(sdkId: "Test")
@aws.auth#sigv4(name: "test-service")
service RequiredValues {
    operations: [TestOperation]
}

operation TestOperation {
    errors: [Error]
}

@error
structure Error {
    @required
    requestId: String

    @required
    message: String
}
