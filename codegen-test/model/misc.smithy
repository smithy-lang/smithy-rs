$version: "1.0"

namespace aws.protocoltests.misc

use aws.protocols#restJson1

/// A service to test miscellaneous aspects of code generation where protocol
/// selection is not relevant. If you want to test something protocol-specific,
/// add it to a separate `<protocol>-extras.smithy`.
@restJson1
@title("MiscService")
service MiscService {
    operations: [
        OperationWithInnerRequiredShape,
    ],
}

/// To not regress on https://github.com/awslabs/smithy-rs/pull/1266
@http(uri: "/operation", method: "GET")
operation OperationWithInnerRequiredShape {
    input: OperationWithInnerRequiredShapeInput,
    output: OperationWithInnerRequiredShapeOutput,
}

structure OperationWithInnerRequiredShapeInput {
    inner: InnerShape
}

structure InnerShape {
    @required
    requiredInnerMostShape: InnermostShape
}

structure InnermostShape {
    aString: String
}

structure OperationWithInnerRequiredShapeOutput { }
