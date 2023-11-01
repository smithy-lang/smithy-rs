$version: "1.0"

namespace com.amazonaws.ebs

use smithy.test#httpRequestTests
use aws.protocols#restJson1
use com.amazonaws.ebs#PutSnapshotBlock
apply PutSnapshotBlock @httpRequestTests([
    {
        id: "XAmzSha256Present",
        documentation: "EBS sets `unsignedPayload` which means `X-Amz-Sha256` should be set to unsigned payload",
        protocol: restJson1,
        method: "POST",
        uri: "/snapshots/snap-1/blocks/0",
        headers: {
            "x-amz-content-sha256": "UNSIGNED-PAYLOAD",
        },
        body: "1234",
        params: {
            SnapshotId: "snap-1",
            BlockIndex: 0,
            BlockData: "1234",
            Checksum: "1234",
            ChecksumAlgorithm: "SHA256",
            DataLength: 4,
        },
        vendorParams: {
            "endpointParams": {
                "builtInParams": {
                    "AWS::Region": "us-east-1"
                }
            }
        }
    },
])
