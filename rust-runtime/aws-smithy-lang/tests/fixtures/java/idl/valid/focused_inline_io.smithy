$version: "2"
$operationOutputSuffix: "Result"
namespace ex

service Service {
    operations: [Write, Read, Empty]
}

// Output is written before input; generated shapes still precede the operation.
operation Write {
    output := @sensitive {
        id: String
    }
    input := @documentation("input docs") {
        @required
        name: String = "default"
    }
    errors: [Failure]
}

@readonly
operation Read {
    input: ReadInput
    output := {}
}

operation Empty {}

structure ReadInput {}

@error("client")
structure Failure {}
