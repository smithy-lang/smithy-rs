$version: "1.0"

namespace aws.protocoltests.json

use smithy.rules#endpointRuleSet
use smithy.rules#endpointTests

use smithy.rules#clientContextParams
use smithy.rules#staticContextParams
use smithy.rules#contextParam
use aws.protocols#awsJson1_1

@awsJson1_1
@endpointRuleSet({
    "version": "1.0",
    "rules": [{
                  "type": "endpoint",
                  "conditions": [],
                  "endpoint": { "url": "https://www.example.com" }
              }],
    "parameters": {
        "Bucket": { "required": false, "type": "String" },
        "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
    }
})
service TestService {
    operations: [TestOperation]
}

operation TestOperation {
    input: TestOperationInput
}

structure TestOperationInput {
    @contextParam(name: "Bucket")
    bucket: String
}
