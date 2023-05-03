$version: "2"
namespace com.amazonaws.testservice

use aws.api#service
use aws.protocols#restJson1
use smithy.rules#endpointRuleSet
use smithy.rules#endpointTests

@endpointRuleSet({
    "version": "1.0",
    "parameters": {
        "Region": {
            "required": true,
            "type": "String",
            "builtIn": "AWS::Region",
            "default": "us-east-2",
        },
    },
    "rules": [
        {
            "type": "endpoint",
            "conditions": [],
            "endpoint": {
                "url": "https://prod.{Region}.api.myservice.aws.dev",
                "properties": {
                    "authSchemes": [{
                                        "name": "sigv4",
                                        "signingRegion": "{Region}",
                                    }]
                }
            }
        }
    ]
})
@endpointTests({"version": "1", "testCases": [
    {
        "documentation": "region should fallback to the default",
        "expect": {
            "endpoint": {
                "url": "https://prod.us-east-2.api.myservice.aws.dev",
                "properties": {
                    "authSchemes": [{
                                        "name": "sigv4",
                                        "signingRegion": "us-east-2",
                                    }]

                }
            }
        },
        "params": { }
        "operationInputs": [
            { "operationName": "TestOperation", "operationParams": {
                "bar": { f: "blah" }
              } }
        ]
    }]
})
@restJson1
@title("Test Service")
@service(sdkId: "Test")
@aws.auth#sigv4(name: "test-service")
service TestService {
    operations: [TestOperation]
}

@input
structure Foo { bar: Bar }

structure Bar { f: String }

@http(uri: "/foo", method: "POST")
operation TestOperation {
    input: Foo
}
