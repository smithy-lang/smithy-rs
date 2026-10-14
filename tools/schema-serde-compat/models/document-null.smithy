$version: "2"
namespace compat.documentnull
use aws.protocols#restJson1
@restJson1
service RestDocumentNull { version: "1", operations: [Payload, StructurePayload, UnionPayload] }
@http(method: "POST", uri: "/payload")
operation Payload {
    input := { @httpPayload document: Document }
    output := { @httpPayload value: String }
}
@http(method: "POST", uri: "/structurepayload")
operation StructurePayload {
    input := { @httpPayload value: PayloadStructure }
}
@http(method: "POST", uri: "/unionpayload")
operation UnionPayload {
    input := { @httpPayload value: PayloadUnion }
}
structure PayloadStructure { value: String }
union PayloadUnion { value: String }
