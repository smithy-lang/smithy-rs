$version: "2.0"

namespace com.aws.example.d3

use aws.protocols#restJson1

/// Service using the legacy experimental
/// `experimentalCustomValidationExceptionWithReasonPleaseDoNotUse` codegen
/// flag path (shape layout mirrors the decorator's expectations: Message /
/// Reason / Fields members). Verifies assumption D3: that path is independent
/// of the @validationException trait path.
@restJson1
service D3CustomReasonService {
    version: "1.0"
    operations: [
        D3Operation
    ]
}

@http(method: "POST", uri: "/d3")
operation D3Operation {
    input := {
        @required
        @length(min: 1, max: 10)
        name: String
    }
    errors: [
        ValidationException
    ]
}

enum ValidationExceptionFieldReason {
    LENGTH_NOT_VALID = "LengthNotValid"
    PATTERN_NOT_VALID = "PatternNotValid"
    SYNTAX_NOT_VALID = "SyntaxNotValid"
    VALUE_NOT_VALID = "ValueNotValid"
    OTHER = "Other"
}

/// Stores information about a field passed inside a request that resulted in an exception.
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

/// The input fails to satisfy the constraints specified by an AWS service.
@error("client")
@httpError(400)
structure ValidationException {
    /// Description of the error.
    @required
    Message: String

    /// Reason the request failed validation.
    @required
    Reason: ValidationExceptionReason

    /// The field that caused the error, if applicable.
    Fields: ValidationExceptionFieldList
}
