$version: "1.0"

namespace com.amazonaws.apigateway

use smithy.rules#endpointRuleSet
apply BackplaneControlService @endpointRuleSet({
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
