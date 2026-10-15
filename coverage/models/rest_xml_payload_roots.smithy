// coverage-service: coverage.payload.restxml#XmlPayloadCoverage
// coverage-plugins: client,server
$version: "2"
namespace coverage.payload.restxml

use aws.protocols#restXml
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests

// These are independent wire expectations. Target metadata supplies the namespace;
// a payload member's xmlName takes precedence over its target's xmlName.
// Absent structured payload cases use explicit null. Omitting body from params
// makes the native test instantiator construct a default present structure.
@restXml
service XmlPayloadCoverage {
    version: "1"
    operations: [TargetPayload, MemberPayload, UnionTargetPayload, UnionMemberPayload]
}

@xmlName("TargetRoot")
@xmlNamespace(uri: "urn:target", prefix: "t")
structure Target {
    @xmlName("Child") child: String
    @xmlName("Items") items: Strings
}
list Strings { @xmlName("Entry") member: String }

structure Record {
    @xmlName("Child") child: String
    @xmlName("Items") items: Strings
}
@xmlName("ChoiceRoot")
@xmlNamespace(uri: "urn:union")
union Choice {
    @xmlName("Text") text: String
    @xmlName("Record") record: Record
    @xmlName("Unit") unit: Unit
}

@http(method: "POST", uri: "/target")
@httpRequestTests([
    { id: "XmlTargetPayloadAbsentRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/target", headers: { "Content-Type": "application/xml" }, body: "", bodyMediaType: "application/xml", params: { body: null } },
    { id: "XmlTargetPayloadEmptyRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/target", headers: { "Content-Type": "application/xml" }, body: "<TargetRoot xmlns:t=\"urn:target\"></TargetRoot>", bodyMediaType: "application/xml", params: { body: {} } },
    { id: "XmlTargetPayloadEmptyCollectionRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/target", headers: { "Content-Type": "application/xml" }, body: "<TargetRoot xmlns:t=\"urn:target\"><Items></Items></TargetRoot>", bodyMediaType: "application/xml", params: { body: { items: [] } } },
    { id: "XmlTargetPayloadPopulatedRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/target", headers: { "Content-Type": "application/xml" }, body: "<TargetRoot xmlns:t=\"urn:target\"><Child>value</Child><Items><Entry>one</Entry><Entry>two</Entry></Items></TargetRoot>", bodyMediaType: "application/xml", params: { body: { child: "value", items: ["one", "two"] } } },
])
@httpResponseTests([
    { id: "XmlTargetPayloadAbsentResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "", bodyMediaType: "application/xml", params: { body: null } },
    { id: "XmlTargetPayloadEmptyResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<TargetRoot xmlns:t=\"urn:target\"></TargetRoot>", bodyMediaType: "application/xml", params: { body: {} } },
    { id: "XmlTargetPayloadEmptyCollectionResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<TargetRoot xmlns:t=\"urn:target\"><Items></Items></TargetRoot>", bodyMediaType: "application/xml", params: { body: { items: [] } } },
    { id: "XmlTargetPayloadPopulatedResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<TargetRoot xmlns:t=\"urn:target\"><Child>value</Child><Items><Entry>one</Entry><Entry>two</Entry></Items></TargetRoot>", bodyMediaType: "application/xml", params: { body: { child: "value", items: ["one", "two"] } } },
])
operation TargetPayload { input: TargetPayloadInput, output: TargetPayloadOutput }
structure TargetPayloadInput { @httpPayload body: Target }
structure TargetPayloadOutput { @httpPayload body: Target }

@http(method: "POST", uri: "/member")
@httpRequestTests([
    { id: "XmlMemberPayloadAbsentRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/member", headers: { "Content-Type": "application/xml" }, body: "", bodyMediaType: "application/xml", params: { body: null } },
    { id: "XmlMemberPayloadEmptyRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/member", headers: { "Content-Type": "application/xml" }, body: "<MemberRoot xmlns:t=\"urn:target\"></MemberRoot>", bodyMediaType: "application/xml", params: { body: {} } },
    { id: "XmlMemberPayloadEmptyCollectionRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/member", headers: { "Content-Type": "application/xml" }, body: "<MemberRoot xmlns:t=\"urn:target\"><Items></Items></MemberRoot>", bodyMediaType: "application/xml", params: { body: { items: [] } } },
    { id: "XmlMemberPayloadPopulatedRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/member", headers: { "Content-Type": "application/xml" }, body: "<MemberRoot xmlns:t=\"urn:target\"><Child>value</Child><Items><Entry>one</Entry><Entry>two</Entry></Items></MemberRoot>", bodyMediaType: "application/xml", params: { body: { child: "value", items: ["one", "two"] } } },
])
@httpResponseTests([
    { id: "XmlMemberPayloadAbsentResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "", bodyMediaType: "application/xml", params: { body: null } },
    { id: "XmlMemberPayloadEmptyResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<MemberRoot xmlns:t=\"urn:target\"></MemberRoot>", bodyMediaType: "application/xml", params: { body: {} } },
    { id: "XmlMemberPayloadEmptyCollectionResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<MemberRoot xmlns:t=\"urn:target\"><Items></Items></MemberRoot>", bodyMediaType: "application/xml", params: { body: { items: [] } } },
    { id: "XmlMemberPayloadPopulatedResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<MemberRoot xmlns:t=\"urn:target\"><Child>value</Child><Items><Entry>one</Entry><Entry>two</Entry></Items></MemberRoot>", bodyMediaType: "application/xml", params: { body: { child: "value", items: ["one", "two"] } } },
])
operation MemberPayload { input: MemberPayloadInput, output: MemberPayloadOutput }
structure MemberPayloadInput { @httpPayload @xmlName("MemberRoot") body: Target }
structure MemberPayloadOutput { @httpPayload @xmlName("MemberRoot") body: Target }

@http(method: "POST", uri: "/union-target")
@httpRequestTests([
    { id: "XmlUnionTargetPayloadAbsentRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/union-target", headers: { "Content-Type": "application/xml" }, body: "", bodyMediaType: "application/xml", params: {} },
    { id: "XmlUnionTargetPayloadEmptyTextRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/union-target", headers: { "Content-Type": "application/xml" }, body: "<ChoiceRoot xmlns=\"urn:union\"><Text></Text></ChoiceRoot>", bodyMediaType: "application/xml", params: { body: { text: "" } } },
    { id: "XmlUnionTargetPayloadPopulatedTextRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/union-target", headers: { "Content-Type": "application/xml" }, body: "<ChoiceRoot xmlns=\"urn:union\"><Text>value</Text></ChoiceRoot>", bodyMediaType: "application/xml", params: { body: { text: "value" } } },
    { id: "XmlUnionTargetPayloadEmptyRecordRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/union-target", headers: { "Content-Type": "application/xml" }, body: "<ChoiceRoot xmlns=\"urn:union\"><Record></Record></ChoiceRoot>", bodyMediaType: "application/xml", params: { body: { record: {} } } },
    { id: "XmlUnionTargetPayloadPopulatedRecordRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/union-target", headers: { "Content-Type": "application/xml" }, body: "<ChoiceRoot xmlns=\"urn:union\"><Record><Child>value</Child><Items><Entry>one</Entry><Entry>two</Entry></Items></Record></ChoiceRoot>", bodyMediaType: "application/xml", params: { body: { record: { child: "value", items: ["one", "two"] } } } },
    { id: "XmlUnionTargetPayloadUnitRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/union-target", headers: { "Content-Type": "application/xml" }, body: "<ChoiceRoot xmlns=\"urn:union\"><Unit></Unit></ChoiceRoot>", bodyMediaType: "application/xml", params: { body: { unit: {} } } },
])
@httpResponseTests([
    { id: "XmlUnionTargetPayloadAbsentResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "", bodyMediaType: "application/xml", params: {} },
    { id: "XmlUnionTargetPayloadEmptyTextResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<ChoiceRoot xmlns=\"urn:union\"><Text></Text></ChoiceRoot>", bodyMediaType: "application/xml", params: { body: { text: "" } } },
    { id: "XmlUnionTargetPayloadPopulatedTextResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<ChoiceRoot xmlns=\"urn:union\"><Text>value</Text></ChoiceRoot>", bodyMediaType: "application/xml", params: { body: { text: "value" } } },
    { id: "XmlUnionTargetPayloadEmptyRecordResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<ChoiceRoot xmlns=\"urn:union\"><Record></Record></ChoiceRoot>", bodyMediaType: "application/xml", params: { body: { record: {} } } },
    { id: "XmlUnionTargetPayloadPopulatedRecordResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<ChoiceRoot xmlns=\"urn:union\"><Record><Child>value</Child><Items><Entry>one</Entry><Entry>two</Entry></Items></Record></ChoiceRoot>", bodyMediaType: "application/xml", params: { body: { record: { child: "value", items: ["one", "two"] } } } },
    { id: "XmlUnionTargetPayloadUnitResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<ChoiceRoot xmlns=\"urn:union\"><Unit></Unit></ChoiceRoot>", bodyMediaType: "application/xml", params: { body: { unit: {} } } },
])
operation UnionTargetPayload { input: UnionTargetPayloadInput, output: UnionTargetPayloadOutput }
structure UnionTargetPayloadInput { @httpPayload body: Choice }
structure UnionTargetPayloadOutput { @httpPayload body: Choice }

@http(method: "POST", uri: "/union-member")
@httpRequestTests([
    { id: "XmlUnionMemberPayloadAbsentRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/union-member", headers: { "Content-Type": "application/xml" }, body: "", bodyMediaType: "application/xml", params: {} },
    { id: "XmlUnionMemberPayloadEmptyTextRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/union-member", headers: { "Content-Type": "application/xml" }, body: "<MemberChoiceRoot xmlns=\"urn:union\"><Text></Text></MemberChoiceRoot>", bodyMediaType: "application/xml", params: { body: { text: "" } } },
    { id: "XmlUnionMemberPayloadPopulatedTextRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/union-member", headers: { "Content-Type": "application/xml" }, body: "<MemberChoiceRoot xmlns=\"urn:union\"><Text>value</Text></MemberChoiceRoot>", bodyMediaType: "application/xml", params: { body: { text: "value" } } },
    { id: "XmlUnionMemberPayloadEmptyRecordRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/union-member", headers: { "Content-Type": "application/xml" }, body: "<MemberChoiceRoot xmlns=\"urn:union\"><Record></Record></MemberChoiceRoot>", bodyMediaType: "application/xml", params: { body: { record: {} } } },
    { id: "XmlUnionMemberPayloadPopulatedRecordRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/union-member", headers: { "Content-Type": "application/xml" }, body: "<MemberChoiceRoot xmlns=\"urn:union\"><Record><Child>value</Child><Items><Entry>one</Entry><Entry>two</Entry></Items></Record></MemberChoiceRoot>", bodyMediaType: "application/xml", params: { body: { record: { child: "value", items: ["one", "two"] } } } },
    { id: "XmlUnionMemberPayloadUnitRequest", protocol: "aws.protocols#restXml", method: "POST", uri: "/union-member", headers: { "Content-Type": "application/xml" }, body: "<MemberChoiceRoot xmlns=\"urn:union\"><Unit></Unit></MemberChoiceRoot>", bodyMediaType: "application/xml", params: { body: { unit: {} } } },
])
@httpResponseTests([
    { id: "XmlUnionMemberPayloadAbsentResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "", bodyMediaType: "application/xml", params: {} },
    { id: "XmlUnionMemberPayloadEmptyTextResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<MemberChoiceRoot xmlns=\"urn:union\"><Text></Text></MemberChoiceRoot>", bodyMediaType: "application/xml", params: { body: { text: "" } } },
    { id: "XmlUnionMemberPayloadPopulatedTextResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<MemberChoiceRoot xmlns=\"urn:union\"><Text>value</Text></MemberChoiceRoot>", bodyMediaType: "application/xml", params: { body: { text: "value" } } },
    { id: "XmlUnionMemberPayloadEmptyRecordResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<MemberChoiceRoot xmlns=\"urn:union\"><Record></Record></MemberChoiceRoot>", bodyMediaType: "application/xml", params: { body: { record: {} } } },
    { id: "XmlUnionMemberPayloadPopulatedRecordResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<MemberChoiceRoot xmlns=\"urn:union\"><Record><Child>value</Child><Items><Entry>one</Entry><Entry>two</Entry></Items></Record></MemberChoiceRoot>", bodyMediaType: "application/xml", params: { body: { record: { child: "value", items: ["one", "two"] } } } },
    { id: "XmlUnionMemberPayloadUnitResponse", protocol: "aws.protocols#restXml", code: 200, headers: { "Content-Type": "application/xml" }, body: "<MemberChoiceRoot xmlns=\"urn:union\"><Unit></Unit></MemberChoiceRoot>", bodyMediaType: "application/xml", params: { body: { unit: {} } } },
])
operation UnionMemberPayload { input: UnionMemberPayloadInput, output: UnionMemberPayloadOutput }
structure UnionMemberPayloadInput { @httpPayload @xmlName("MemberChoiceRoot") body: Choice }
structure UnionMemberPayloadOutput { @httpPayload @xmlName("MemberChoiceRoot") body: Choice }
