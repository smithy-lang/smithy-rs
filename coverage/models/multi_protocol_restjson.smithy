// coverage-service: com.example.multiprotocol#MultiProtocolService
// coverage-plugins: server
// coverage-import: multi-protocol-compat/multi-protocol-common.smithy
$version: "2"
namespace com.example.multiprotocol
@aws.protocols#restJson1
service MultiProtocolService {
    version: "1"
    operations: [StructurePayload, UnionPayload, ResponseBindings, RichData, ModeledDefaults, NullableValues, DocumentValues]
}
