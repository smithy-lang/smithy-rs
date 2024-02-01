$version: "2.0"

namespace aws.protocoltests.rpcv2

use aws.protocoltests.shared#StringList
use smithy.protocols#rpcv2
use smithy.test#httpRequestTests
use smithy.test#httpResponseTests


@httpRequestTests([
    {
        id: "RpcV2CborSimpleScalarProperties",
        protocol: rpcv2,
        documentation: "Serializes simple scalar properties",
        headers: {
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor",
            "Content-Type": "application/cbor"
        }
        method: "POST",
        bodyMediaType: "application/cbor",
        uri: "/service/RpcV2Protocol/operation/SimpleScalarProperties",
        body: "v2lieXRlVmFsdWUFa2RvdWJsZVZhbHVl+z/+OVgQYk3TcWZhbHNlQm9vbGVhblZhbHVl9GpmbG9hdFZhbHVl+kDz989saW50ZWdlclZhbHVlGQEAaWxvbmdWYWx1ZRkmkWpzaG9ydFZhbHVlGSaqa3N0cmluZ1ZhbHVlZnNpbXBsZXB0cnVlQm9vbGVhblZhbHVl9f8="
        params: {
            trueBooleanValue: true,
            falseBooleanValue: false,
            byteValue: 5,
            doubleValue: 1.889,
            floatValue: 7.624,
            integerValue: 256,
            shortValue: 9898,
            longValue: 9873
            stringValue: "simple"
        }
    },
    {
        id: "RpcV2CborClientDoesntSerializeNullStructureValues",
        documentation: "RpcV2 Cbor should not serialize null structure values",
        protocol: rpcv2,
        method: "POST",
        uri: "/service/RpcV2Protocol/operation/SimpleScalarProperties",
        body: "v/8=",
        bodyMediaType: "application/cbor",
        headers: {
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor",
            "Content-Type": "application/cbor"
        }
        params: {
            stringValue: null
        },
        appliesTo: "client"
    },
    {
        id: "RpcV2CborServerDoesntDeSerializeNullStructureValues",
        documentation: "RpcV2 Cbor should not deserialize null structure values",
        protocol: rpcv2,
        method: "POST",
        uri: "/service/RpcV2Protocol/operation/SimpleScalarProperties",
        body: "v2tzdHJpbmdWYWx1Zfb/",
        bodyMediaType: "application/cbor",
        headers: {
            "smithy-protocol": "rpc-v2-cbor",
            "Accept": "application/cbor",
            "Content-Type": "application/cbor"
        }
        params: {},
        appliesTo: "server"
    },
])
@httpResponseTests([
    {
        id: "simple_scalar_structure",
        protocol: rpcv2,
        documentation: "Serializes simple scalar properties",
        headers: {
            "smithy-protocol": "rpc-v2-cbor",
            "Content-Type": "application/cbor"
        }
        bodyMediaType: "application/cbor",
        body: "v2lieXRlVmFsdWUFa2RvdWJsZVZhbHVl+z/+OVgQYk3TcWZhbHNlQm9vbGVhblZhbHVl9GpmbG9hdFZhbHVl+kDz989saW50ZWdlclZhbHVlGQEAaWxvbmdWYWx1ZRkmkWpzaG9ydFZhbHVlGSaqa3N0cmluZ1ZhbHVlZnNpbXBsZXB0cnVlQm9vbGVhblZhbHVl9f8=",
        code: 200,
        params: {
            trueBooleanValue: true,
            falseBooleanValue: false,
            byteValue: 5,
            doubleValue: 1.889,
            floatValue: 7.624,
            integerValue: 256,
            shortValue: 9898,
            stringValue: "simple"
        }
    },
    {
        id: "RpcV2CborClientDoesntDeSerializeNullStructureValues",
        documentation: "RpcV2 Cbor should deserialize null structure values",
        protocol: rpcv2,
        body: "v2tzdHJpbmdWYWx1Zfb/",
        code: 200,
        bodyMediaType: "application/cbor",
        headers: {
            "smithy-protocol": "rpc-v2-cbor",
            "Content-Type": "application/cbor"
        }
        params: {}
        appliesTo: "client"
    },
    {
        id: "RpcV2CborServerDoesntSerializeNullStructureValues",
        documentation: "RpcV2 Cbor should not serialize null structure values",
        protocol: rpcv2,
        body: "v/8=",
        code: 200,
        bodyMediaType: "application/cbor",
        headers: {
            "smithy-protocol": "rpc-v2-cbor",
            "Content-Type": "application/cbor"
        }
        params: {
            stringValue: null
        },
        appliesTo: "server"
    },
])
operation SimpleScalarProperties {
    input: SimpleScalarStructure,
    output: SimpleScalarStructure
}


structure SimpleScalarStructure {
    trueBooleanValue: Boolean,
    falseBooleanValue: Boolean,
    byteValue: Byte,
    doubleValue: Double,
    floatValue: Float,
    integerValue: Integer,
    longValue: Long,
    shortValue: Short,
    stringValue: String,
}
