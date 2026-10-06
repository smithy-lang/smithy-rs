// coverage-service: test.customvalidationmin#CustomValidationMinimal
// coverage-plugins: server
$version: "2.0"

namespace test.customvalidationmin

use aws.protocols#restJson1
use smithy.framework.rust#validationException
use smithy.framework.rust#validationMessage

@restJson1
service CustomValidationMinimal {
    version: "1.0"
    operations: [PutThing]
    errors: [MinimalValidationException]
}

@http(method: "POST", uri: "/thing")
operation PutThing {
    input := {
        @required
        @length(min: 1, max: 10)
        name: String

        @range(min: 1, max: 100)
        count: Integer
    }
    output := {
        ok: Boolean
    }
}

// A message-only custom validation exception (no @validationFieldList member) exercises
// the field-list-absent code paths in UserProvidedValidationExceptionDecorator.
@error("client")
@httpError(400)
@validationException
structure MinimalValidationException {
    @required
    @validationMessage
    customMessage: String
}
