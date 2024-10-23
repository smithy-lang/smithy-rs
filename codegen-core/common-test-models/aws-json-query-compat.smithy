$version: "1.0"

namespace aws.protocoltests.misc

use aws.protocols#awsQueryCompatible
use aws.protocols#awsJson1_0
use aws.protocols#awsQueryError
use smithy.test#httpRequestTests
@awsQueryCompatible
@awsJson1_0
service QueryCompatService {
    operations: [
        Operation
    ]
}

@httpRequestTests([{
    id: "BasicQueryCompatTest"
    protocol: awsJson1_0,
    method: "POST",
    uri: "/",
    body: "{\"message\":\"hello!\"}",
    bodyMedaType: "application/json",
    params: {
        message: "hello!"
    },
    headers: {
        "x-amz-target": "QueryCompatService.Operation",
        "x-amzn-query-mode": "true",
    }
   }
])
operation Operation {
    input: OperationInputOutput
    output: OperationInputOutput
}

structure OperationInputOutput {
    message: String
}
