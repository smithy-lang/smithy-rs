// coverage-service: com.example.multiprotocol#MultiProtocolService
// coverage-plugins: server
// coverage-import: multi-protocol-common.smithy
$version: "2"
namespace com.example.multiprotocol
@smithy.protocols#rpcv2Cbor
service MultiProtocolService {
    version: "1"
    operations: [StructurePayload, UnionPayload, ResponseBindings, RichData, ModeledDefaults, NullableValues]
}
