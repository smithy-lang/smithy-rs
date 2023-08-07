// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// The API Gateway model is coming from Smithy's protocol tests, and includes an `Accept` header test:
// https://github.com/awslabs/smithy/blob/2f6553ff39e6bba9edc644ef5832661821785319/smithy-aws-protocol-tests/model/restJson1/services/apigateway.smithy#L30-L43

$version: "1.0"

namespace com.amazonaws.apigateway

use smithy.rules#endpointRuleSet

// Add an endpoint ruleset to the Smithy protocol test API Gateway model so that the code generator doesn't fail
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
