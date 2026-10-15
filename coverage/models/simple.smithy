// coverage-service: com.amazonaws.simple#SimpleService
// coverage-import: simple.smithy
$version: "2.0"
namespace com.amazonaws.simple
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

apply Operation @httpRequestTests([
    { id: "SimpleAbsentRequest", protocol: "aws.protocols#restJson1", method: "POST", uri: "/operation", headers: { "Content-Type": "application/json" }, body: "{}", bodyMediaType: "application/json", params: {} },
    { id: "SimpleEmptyStringRequest", protocol: "aws.protocols#restJson1", method: "POST", uri: "/operation", headers: { "Content-Type": "application/json" }, body: "{\"message\":\"\"}", bodyMediaType: "application/json", params: { message: "" } },
    { id: "SimplePopulatedRequest", protocol: "aws.protocols#restJson1", method: "POST", uri: "/operation", headers: { "Content-Type": "application/json" }, body: "{\"message\":\"hello \\\"world\\\"\\npath\\\\snowman ☃\"}", bodyMediaType: "application/json", params: { message: "hello \"world\"\npath\\snowman \u2603" } },
])
apply Operation @httpResponseTests([
    { id: "SimpleAbsentResponse", protocol: "aws.protocols#restJson1", code: 200, headers: { "Content-Type": "application/json" }, body: "{}", bodyMediaType: "application/json", params: {} },
    { id: "SimpleEmptyStringResponse", protocol: "aws.protocols#restJson1", code: 200, headers: { "Content-Type": "application/json" }, body: "{\"message\":\"\"}", bodyMediaType: "application/json", params: { message: "" } },
    { id: "SimplePopulatedResponse", protocol: "aws.protocols#restJson1", code: 200, headers: { "Content-Type": "application/json" }, body: "{\"message\":\"hello \\\"world\\\"\\npath\\\\snowman ☃\"}", bodyMediaType: "application/json", params: { message: "hello \"world\"\npath\\snowman \u2603" } },
])
