$version: "2.0"

namespace com.aws.example.distinctns

use aws.protocols#awsJson1_0
use com.custom.errors#DistinctNsValidationException

/// Service in one namespace using a custom @validationException shape from a
/// different namespace (com.custom.errors). Verifies assumption B5 on a
/// namespace-carrying protocol (awsJson1.0 puts the full shape ID in `__type`).
@awsJson1_0
service DistinctNsValidationExample {
    version: "1.0.0"
    operations: [
        TestOperation
    ]
    errors: [
        DistinctNsValidationException
    ]
}

@http(method: "POST", uri: "/test")
operation TestOperation {
    input: TestInput
}

structure TestInput {
    @required
    @length(min: 1, max: 10)
    name: String

    @range(min: 1, max: 100)
    age: Integer
}
