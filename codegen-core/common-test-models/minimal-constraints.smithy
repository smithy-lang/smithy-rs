$version: "1.0"

namespace com.amazonaws.constraints

use aws.protocols#restJson1

/// A service to test aspects of code generation where shapes have constraint traits.
@restJson1
@title("ConstraintsService")
service ConstraintsService {
    operations: [
        ConstrainedHttpBoundShapesOperation,
    ],
}

@http(
    uri: "/constrained-http-bound-shapes-operation/{rangeIntegerLabel}/{rangeShortLabel}/{rangeLongLabel}/{rangeByteLabel}/{lengthStringLabel}",
    method: "POST"
)
operation ConstrainedHttpBoundShapesOperation {
    input: ConstrainedHttpBoundShapesOperationInputOutput,
    output: ConstrainedHttpBoundShapesOperationInputOutput,
    errors: []
}

structure ConstrainedHttpBoundShapesOperationInputOutput {
    @required
    @httpLabel
    lengthStringLabel: String,

    @required
    @httpLabel
    rangeIntegerLabel: Integer,

    @required
    @httpLabel
    rangeShortLabel: Short,

    @required
    @httpLabel
    rangeLongLabel: Long,

    @required
    @httpLabel
    rangeByteLabel: Byte,
}
