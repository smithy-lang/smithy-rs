$version: "2"

namespace smithy.rust.codegen.compatibility

use aws.protocols#awsJson1_0
use aws.protocols#awsJson1_1
use aws.protocols#awsQuery
use aws.protocols#ec2Query
use aws.protocols#restXml
use smithy.protocols#rpcv2Cbor

@restXml
service RestXmlService {
    version: "2024-01-01"
    operations: [RestOperation]
}

@awsJson1_0
service AwsJson10Service {
    version: "2024-01-01"
    operations: [RpcOperation]
}

@awsJson1_1
service AwsJson11Service {
    version: "2024-01-01"
    operations: [RpcOperation]
}

@xmlNamespace(uri: "https://example.com/aws-query")
@awsQuery
service AwsQueryService {
    version: "2024-01-01"
    operations: [RpcOperation]
}

@xmlNamespace(uri: "https://example.com/ec2-query")
@ec2Query
service Ec2QueryService {
    version: "2024-01-01"
    operations: [RpcOperation]
}

@rpcv2Cbor
service RpcV2CborService {
    version: "2024-01-01"
    operations: [RpcOperation]
}

@http(uri: "/compatibility", method: "POST")
operation RestOperation {
    input := {
        value: String
    }
    output := {
        value: String
    }
    errors: [CompatibilityError]
}

operation RpcOperation {
    input := {
        value: String
    }
    output := {
        value: String
    }
    errors: [CompatibilityError]
}

@error("client")
structure CompatibilityError {
    message: String
}
