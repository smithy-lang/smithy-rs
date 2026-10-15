// coverage-service: casing#ACRONYMInside_Service
// coverage-import: naming-obstacle-course-casing.smithy
$version: "2.0"
namespace casing
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

// The only active imported operation has no modeled input or output.
// Keep its acronym/underscore service name on the wire. Populated casing cases
// live in the separate naming_casing_populated coverage-only model.
apply DoNothing @httpRequestTests([
    { id: "NamingCasingDoNothingRequest", protocol: "aws.protocols#awsJson1_1", method: "POST", uri: "/", headers: { "Content-Type": "application/x-amz-json-1.1", "X-Amz-Target": "ACRONYMInside_Service.DoNothing" }, body: "", bodyMediaType: "application/json", params: {} },
])
apply DoNothing @httpResponseTests([
    { id: "NamingCasingDoNothingResponse", protocol: "aws.protocols#awsJson1_1", code: 200, headers: { "Content-Type": "application/x-amz-json-1.1" }, body: "", bodyMediaType: "application/json", params: {} },
])
