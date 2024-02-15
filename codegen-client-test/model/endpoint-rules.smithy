$version: "1.0"

namespace aws.protocoltests.json

use aws.protocols#awsJson1_1
use smithy.rules#contextParam
use smithy.rules#endpointRuleSet

@awsJson1_1
@endpointRuleSet({
    version: "1.0"
    rules: [
        {
            type: "endpoint"
            conditions: []
            endpoint: { url: "https://www.example.com" }
        }
    ]
    parameters: {
        Bucket: { required: false, type: "String" }
    }
})
service TestService {
    operations: [
        TestOperation
    ]
}

operation TestOperation {
    input: TestOperationInput
}

structure TestOperationInput {
    @contextParam(name: "Bucket")
    bucket: String
}
