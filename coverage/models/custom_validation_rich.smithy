// coverage-service: test.customvalidationrich#CustomValidationRich
// coverage-plugins: server
$version: "2.0"

namespace test.customvalidationrich

use aws.protocols#restJson1
use smithy.framework.rust#validationException
use smithy.framework.rust#validationFieldList
use smithy.framework.rust#validationFieldMessage
use smithy.framework.rust#validationFieldName
use smithy.framework.rust#validationMessage

@restJson1
service CustomValidationRich {
    version: "1.0"
    operations: [PutWidget]
    errors: [MyValidationException]
}

@http(method: "POST", uri: "/widget/{id}")
operation PutWidget {
    input := {
        @required
        @httpLabel
        @pattern("^[a-z0-9-]+$")
        id: String

        @length(min: 2, max: 64)
        title: PatternString

        @range(min: -10, max: 10)
        offsetByte: Byte

        @range(min: 0, max: 1000)
        offsetShort: Short

        @range(min: 0)
        offsetLong: Long

        color: Color

        rating: Rating

        @length(min: 1, max: 3)
        tags: TagList

        attributes: BoundedMap

        payload: SizedBlob

        uniqueNumbers: UniqueIntList

        choice: ConstrainedUnion
    }
    output := {
        ok: Boolean
    }
}

@pattern("^[A-Za-z ]+$")
string PatternString

enum Color {
    RED = "red"
    GREEN = "green"
}

@range(min: 1, max: 5)
intEnum Rating {
    ONE = 1
    FIVE = 5
}

@length(min: 1, max: 5)
list TagList {
    member: PatternString
}

@length(min: 1, max: 4)
map BoundedMap {
    key: PatternString
    value: PatternString
}

@length(min: 1, max: 1024)
blob SizedBlob

@uniqueItems
list UniqueIntList {
    member: Integer
}

union ConstrainedUnion {
    pattern: PatternString
    tags: TagList
}

enum Severity {
    LOW = "low"
    HIGH = "high"
}

@error("client")
@httpError(400)
@validationException
structure MyValidationException {
    @required
    @validationMessage
    customMessage: String

    @validationFieldList
    customFieldList: CustomValidationFieldList

    // members with defaults exercise the decorator's default-field-assignment branches
    @default("client")
    source: String

    @default(true)
    retryable: Boolean

    @default(3)
    attempts: Integer

    @default("low")
    severity: Severity
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
