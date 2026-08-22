$version: "2.0"

namespace com.aws.example.d1

use aws.protocols#restJson1

/// Service with constrained operation inputs but NO ValidationException
/// declared anywhere in the model, and one operation reachable only through
/// a resource. Verifies assumption D1: auto-injection is the default, the
/// injector walks resources, and the shape is constructed programmatically
/// when absent from the model.
@restJson1
service D1InjectionService {
    version: "1.0"
    resources: [
        Widget
    ]
    operations: [
        TopOp
    ]
}

resource Widget {
    identifiers: {
        id: String
    }
    read: GetWidget
}

@readonly
@http(method: "GET", uri: "/widget/{id}")
operation GetWidget {
    input := {
        @required
        @httpLabel
        @length(min: 1, max: 8)
        id: String
    }
    output := {
        name: String
    }
}

@http(method: "POST", uri: "/top")
operation TopOp {
    input := {
        @range(min: 1, max: 5)
        count: Integer
    }
}
