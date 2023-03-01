$version: "2.0"

namespace com.amazonaws.simple

use smithy.protocols#rpcv2

@rpcv2(format: ["cbor"])
service RpcV2Service {
    version: "SomeVersion",
}
