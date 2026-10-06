// coverage-service: aws.protocoltests.restxml#RestXmlExtras
// coverage-import: codegen-client-test/model/rest-xml-extras.smithy
// coverage-plugins: client
$version: "2.0"

metadata suppressions = [
    {
        id: "HttpMethodSemantics.UnexpectedPayload",
        namespace: "*",
        reason: "Upstream test model intentionally binds payload members to GET",
    },
]
