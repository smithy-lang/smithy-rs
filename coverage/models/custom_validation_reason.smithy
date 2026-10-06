// coverage-service: test.customvalidation#CustomValidationReasonService
// coverage-plugins: server
// coverage-codegen: "experimentalCustomValidationExceptionWithReasonPleaseDoNotUse": "test.customvalidation#ValidationException"
$version: "2.0"

namespace test.customvalidation

use aws.protocols#restJson1

@restJson1
service CustomValidationReasonService {
    version: "1.0"
    operations: [CreateItem]
}

@http(method: "POST", uri: "/item")
operation CreateItem {
    input := {
        @required
        @length(min: 1, max: 10)
        name: SizedString

        @range(min: 1, max: 100)
        count: Integer

        color: Color

        tags: TagList

        payload: SizedBlob

        attributes: BoundedMap

        uniqueNumbers: UniqueIntList

        choice: ConstrainedUnion
    }
    output := {
        id: String
    }
    errors: [ValidationException]
}

@length(min: 1, max: 10)
@pattern("^[a-z]+$")
string SizedString

enum Color {
    RED = "red"
    BLUE = "blue"
}

@length(min: 1, max: 5)
list TagList {
    member: SizedString
}

@length(min: 1, max: 1024)
blob SizedBlob

@length(min: 1, max: 4)
map BoundedMap {
    key: SizedString
    value: SizedString
}

@uniqueItems
list UniqueIntList {
    member: Integer
}

union ConstrainedUnion {
    sized: SizedString
    counted: TagList
}

enum ValidationExceptionFieldReason {
    LENGTH_NOT_VALID = "LengthNotValid"
    PATTERN_NOT_VALID = "PatternNotValid"
    SYNTAX_NOT_VALID = "SyntaxNotValid"
    VALUE_NOT_VALID = "ValueNotValid"
    OTHER = "Other"
}

structure ValidationExceptionField {
    @required
    Name: String

    @required
    Reason: ValidationExceptionFieldReason

    @required
    Message: String
}

list ValidationExceptionFieldList {
    member: ValidationExceptionField
}

enum ValidationExceptionReason {
    FIELD_VALIDATION_FAILED = "FieldValidationFailed"
    UNKNOWN_OPERATION = "UnknownOperation"
    CANNOT_PARSE = "CannotParse"
    OTHER = "Other"
}

@error("client")
@httpError(400)
structure ValidationException {
    @required
    Message: String

    @required
    Reason: ValidationExceptionReason

    Fields: ValidationExceptionFieldList
}
