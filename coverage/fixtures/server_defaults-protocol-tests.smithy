$version: "2"
namespace coverage.fixtures.server_defaults

// Native assertions use populated fixture values and declared HTTP/JSON bindings.
// Constraint status/header assertions reject invalid input before handler invocation.
// Diagnostic strings assert the repository's validation-exception contract;
// their exact wording is not a universal Smithy specification requirement.

apply test.defaults#PutDefaults @smithy.test#httpRequestTests([
    {
        "id": "CoverageServerDefaultsOmittedRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/defaults",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{}",
        "bodyMediaType": "application/json",
        "params": {
            "isoTimestamp": 1625734800,
            "epochTimestamp": 1625734800,
            "emptyList": [],
            "emptyMap": {},
            "boolDoc": true,
            "stringDoc": "hello",
            "posIntDoc": 7,
            "negIntDoc": -7,
            "floatDoc": 1.5,
            "defaultString": "hi",
            "defaultBool": false,
            "defaultInt": 9,
            "defaultDouble": 1.25,
            "defaultBlob": "QUJD",
            "defaultEnum": "red",
            "defaultIntEnum": 2
        },
        "appliesTo": "server"
    },
    {
        "id": "CoverageServerDefaultsExplicitDocumentNullRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/defaults",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"nullDoc\":null}",
        "bodyMediaType": "application/json",
        "params": {
            "isoTimestamp": 1625734800,
            "epochTimestamp": 1625734800,
            "emptyList": [],
            "emptyMap": {},
            "boolDoc": true,
            "stringDoc": "hello",
            "posIntDoc": 7,
            "negIntDoc": -7,
            "floatDoc": 1.5,
            "defaultString": "hi",
            "defaultBool": false,
            "defaultInt": 9,
            "defaultDouble": 1.25,
            "defaultBlob": "QUJD",
            "defaultEnum": "red",
            "defaultIntEnum": 2,
            "nullDoc": null
        },
        "appliesTo": "server"
    },
    {
        "id": "CoverageServerDefaultsOverriddenRequest",
        "documentation": "Populated request with explicitly modeled expected input values.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/defaults",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"isoTimestamp\":1625734801,\"epochTimestamp\":1625734802,\"emptyList\":[\"first\",\"second\"],\"emptyMap\":{\"first\":\"value\"},\"nullDoc\":{\"nested\":[true,17,\"value\"]},\"boolDoc\":false,\"stringDoc\":\"other\",\"posIntDoc\":23,\"negIntDoc\":-23,\"floatDoc\":2.5,\"defaultString\":\"changed\",\"defaultBool\":true,\"defaultInt\":17,\"defaultDouble\":3.75,\"defaultBlob\":\"YmxvYg==\",\"defaultEnum\":\"blue\",\"defaultIntEnum\":1}",
        "bodyMediaType": "application/json",
        "params": {
            "isoTimestamp": 1625734801,
            "epochTimestamp": 1625734802,
            "emptyList": [
                "first",
                "second"
            ],
            "emptyMap": {
                "first": "value"
            },
            "nullDoc": {
                "nested": [
                    true,
                    17,
                    "value"
                ]
            },
            "boolDoc": false,
            "stringDoc": "other",
            "posIntDoc": 23,
            "negIntDoc": -23,
            "floatDoc": 2.5,
            "defaultString": "changed",
            "defaultBool": true,
            "defaultInt": 17,
            "defaultDouble": 3.75,
            "defaultBlob": "blob",
            "defaultEnum": "blue",
            "defaultIntEnum": 1
        },
        "appliesTo": "server"
    }
])

apply test.defaults#PutDefaults @smithy.test#httpResponseTests([
    {
        "id": "CoverageServerDefaultsPopulatedResponse",
        "documentation": "Response bytes derived from the modeled binding and populated fixture values.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"ok\":true}",
        "bodyMediaType": "application/json",
        "params": {
            "ok": true
        },
        "appliesTo": "server"
    }
])
