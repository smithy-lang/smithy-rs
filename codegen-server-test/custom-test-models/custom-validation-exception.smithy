$version: "2.0"

namespace com.aws.example

use aws.protocols#restJson1
use smithy.rust.codegen.server.traits#validationException
use smithy.rust.codegen.server.traits#validationFieldList
use smithy.rust.codegen.server.traits#validationFieldMessage
use smithy.rust.codegen.server.traits#validationFieldName
use smithy.rust.codegen.server.traits#validationMessage

@restJson1
service CustomValidationExample {
    version: "1.0.0"
    operations: [
        TestOperation
    ]
    errors: [
        MyCustomValidationException
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

@error("client")
@httpError(400)
@validationException
structure MyCustomValidationException {
    @required
    @validationMessage
    customMessage: String

    @required
    @default("testReason1")
    reason: ValidationExceptionReason

    @validationFieldList
    customFieldList: CustomValidationFieldList
}

enum ValidationExceptionReason {
    TEST_REASON_0 = "testReason0"
    TEST_REASON_1 = "testReason1"
}

structure CustomValidationField {
    @required
    @validationFieldName
    customFieldName: String

    @required
    @validationFieldMessage
    customFieldMessage: String
}

list CustomValidationFieldList {
    member: CustomValidationField
}
