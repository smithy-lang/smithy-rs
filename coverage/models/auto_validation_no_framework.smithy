// coverage-service: test.autovalidation#AutoValidationService
// coverage-plugins: server
// coverage-transforms: [{"name": "excludeShapesBySelector", "args": {"selector": "[id|namespace = 'smithy.framework']"}}]
$version: "2.0"

namespace test.autovalidation

use aws.protocols#restJson1

// A constrained operation input with no ValidationException in `errors`, in a projection where
// `smithy.framework#ValidationException` has been removed from the model entirely: the
// AttachValidationExceptionToConstrainedOperationInputs transformer must create the shape
// programmatically and attach it.
@restJson1
service AutoValidationService {
    version: "1.0"
    operations: [MakeIt]
}

@http(method: "POST", uri: "/make")
operation MakeIt {
    input := {
        @required
        @length(min: 1, max: 12)
        name: String

        @range(min: 0, max: 10)
        level: Integer
    }
    output := {
        ok: Boolean
    }
}
