$version: "2.0"

namespace com.amazonaws.simple

use aws.protocols#restJson1
use smithy.framework#ValidationException

@restJson1
service SimpleService {
    version: "2022-01-01",
    operations: [
        MyOperation,
    ],
}

@http(method: "POST", uri: "/my-operation")
operation MyOperation {
    input: MyOperationInputOutput,
    output: MyOperationInputOutput,
    errors: [ValidationException]
}

structure MyOperationInputOutput {
    @required
    defaultNullAndRequired: String = null

    @default("ab")
    defaultLengthString: DefaultLengthString,

    @required
    lengthString: LengthString,
}

@length(min: 69)
string LengthString

@length(max: 69)
@default("ab")
string DefaultLengthString
