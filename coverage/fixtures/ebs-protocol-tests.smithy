$version: "2"
namespace coverage.fixtures.ebs

// Independent wire expectations follow the imported model bindings and populated values.
// Streaming event unions require dedicated frame fixtures; these traits do not waive failures.

apply com.amazonaws.ebs#StartSnapshot @smithy.test#httpRequestTests([
    {
        "id": "CoverageEbsStartSnapshotPopulatedRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/snapshots",
        "params": {
            "VolumeSize": 128,
            "Timeout": 30,
            "ClientToken": "client-token-01",
            "Description": "coverage snapshot",
            "Encrypted": true,
            "KmsKeyArn": "arn:aws:kms:us-east-1:123456789012:key/12345678-1234-1234-1234-123456789012",
            "Tags": [
                {
                    "Key": "environment",
                    "Value": "test"
                },
                {
                    "Key": "team",
                    "Value": "storage"
                }
            ]
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"VolumeSize\":128,\"Timeout\":30,\"ClientToken\":\"client-token-01\",\"Description\":\"coverage snapshot\",\"Encrypted\":true,\"KmsKeyArn\":\"arn:aws:kms:us-east-1:123456789012:key/12345678-1234-1234-1234-123456789012\",\"Tags\":[{\"Key\":\"environment\",\"Value\":\"test\"},{\"Key\":\"team\",\"Value\":\"storage\"}]}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "CoverageEbsStartSnapshotRequiredMinimumRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/snapshots",
        "params": {
            "VolumeSize": 1
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"VolumeSize\":1}",
        "bodyMediaType": "application/json"
    },
    {
        "id": "CoverageEbsStartSnapshotFalseRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/snapshots",
        "params": {
            "VolumeSize": 1,
            "Encrypted": false
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        },
        "body": "{\"VolumeSize\":1,\"Encrypted\":false}",
        "bodyMediaType": "application/json"
    }
])

apply com.amazonaws.ebs#StartSnapshot @smithy.test#httpResponseTests([
    {
        "id": "CoverageEbsStartSnapshotPopulatedResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "params": {
            "SnapshotId": "snap-02",
            "ParentSnapshotId": "snap-01",
            "VolumeSize": 128,
            "BlockSize": 524288,
            "StartTime": 1700000000.25,
            "Status": "pending",
            "Description": "coverage snapshot",
            "OwnerId": "123456789012",
            "KmsKeyArn": "arn:aws:kms:us-east-1:123456789012:key/12345678-1234-1234-1234-123456789012",
            "Tags": [
                {
                    "Key": "environment",
                    "Value": "test"
                },
                {
                    "Key": "team",
                    "Value": "storage"
                }
            ]
        },
        "body": "{\"SnapshotId\":\"snap-02\",\"ParentSnapshotId\":\"snap-01\",\"VolumeSize\":128,\"BlockSize\":524288,\"StartTime\":1700000000.25,\"Status\":\"pending\",\"Description\":\"coverage snapshot\",\"OwnerId\":\"123456789012\",\"KmsKeyArn\":\"arn:aws:kms:us-east-1:123456789012:key/12345678-1234-1234-1234-123456789012\",\"Tags\":[{\"Key\":\"environment\",\"Value\":\"test\"},{\"Key\":\"team\",\"Value\":\"storage\"}]}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    },
    {
        "id": "CoverageEbsStartSnapshotEmptyTagsResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "params": {
            "SnapshotId": "snap-01",
            "Tags": [],
            "VolumeSize": 1,
            "Status": "completed"
        },
        "body": "{\"SnapshotId\":\"snap-01\",\"Tags\":[],\"VolumeSize\":1,\"Status\":\"completed\"}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    }
])

apply com.amazonaws.ebs#CompleteSnapshot @smithy.test#httpRequestTests([
    {
        "id": "CoverageEbsCompleteSnapshotPopulatedRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/snapshots/completion/snap-01",
        "params": {
            "SnapshotId": "snap-01",
            "ChangedBlocksCount": 2,
            "ChecksumAggregationMethod": "LINEAR",
            "ChecksumAlgorithm": "SHA256",
            "Checksum": "YWJjZA=="
        },
        "appliesTo": "server",
        "headers": {
            "x-amz-ChangedBlocksCount": "2",
            "x-amz-Checksum-Aggregation-Method": "LINEAR",
            "x-amz-Checksum-Algorithm": "SHA256",
            "x-amz-Checksum": "YWJjZA=="
        },
        "body": ""
    },
    {
        "id": "CoverageEbsCompleteSnapshotZeroCountRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "POST",
        "uri": "/snapshots/completion/snap-01",
        "params": {
            "SnapshotId": "snap-01",
            "ChangedBlocksCount": 0
        },
        "appliesTo": "server",
        "headers": {
            "x-amz-ChangedBlocksCount": "0"
        },
        "body": ""
    }
])

apply com.amazonaws.ebs#CompleteSnapshot @smithy.test#httpResponseTests([
    {
        "id": "CoverageEbsCompleteSnapshotStatusResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 202,
        "params": {
            "Status": "completed"
        },
        "body": "{\"Status\":\"completed\"}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    }
])

apply com.amazonaws.ebs#ListChangedBlocks @smithy.test#httpRequestTests([
    {
        "id": "CoverageEbsListChangedBlocksPopulatedRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/snapshots/snap-02/changedblocks",
        "params": {
            "FirstSnapshotId": "snap-01",
            "SecondSnapshotId": "snap-02",
            "NextToken": "YWJjZA==",
            "MaxResults": 100,
            "StartingBlockIndex": 0
        },
        "appliesTo": "server",
        "queryParams": [
            "firstSnapshotId=snap-01",
            "pageToken=YWJjZA%3D%3D",
            "maxResults=100",
            "startingBlockIndex=0"
        ],
        "body": ""
    }
])

apply com.amazonaws.ebs#ListChangedBlocks @smithy.test#httpResponseTests([
    {
        "id": "CoverageEbsListChangedBlocksPopulatedResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "VolumeSize": 128,
            "BlockSize": 524288,
            "ExpiryTime": 1700000000.25,
            "NextToken": "YWJjZA==",
            "ChangedBlocks": [
                {
                    "BlockIndex": 0,
                    "FirstBlockToken": "YQ==",
                    "SecondBlockToken": "Yg=="
                },
                {
                    "BlockIndex": 7,
                    "SecondBlockToken": "Yw=="
                }
            ]
        },
        "body": "{\"VolumeSize\":128,\"BlockSize\":524288,\"ExpiryTime\":1700000000.25,\"NextToken\":\"YWJjZA==\",\"ChangedBlocks\":[{\"BlockIndex\":0,\"FirstBlockToken\":\"YQ==\",\"SecondBlockToken\":\"Yg==\"},{\"BlockIndex\":7,\"SecondBlockToken\":\"Yw==\"}]}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    }
])

apply com.amazonaws.ebs#ListSnapshotBlocks @smithy.test#httpRequestTests([
    {
        "id": "CoverageEbsListSnapshotBlocksPopulatedRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/snapshots/snap-01/blocks",
        "params": {
            "SnapshotId": "snap-01",
            "NextToken": "YWJjZA==",
            "MaxResults": 10000,
            "StartingBlockIndex": 7
        },
        "appliesTo": "server",
        "queryParams": [
            "pageToken=YWJjZA%3D%3D",
            "maxResults=10000",
            "startingBlockIndex=7"
        ],
        "body": ""
    },
    {
        "id": "CoverageEbsListSnapshotBlocksRequiredOnlyRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/snapshots/snap-01/blocks",
        "params": {
            "SnapshotId": "snap-01"
        },
        "appliesTo": "server",
        "body": ""
    }
])

apply com.amazonaws.ebs#ListSnapshotBlocks @smithy.test#httpResponseTests([
    {
        "id": "CoverageEbsListSnapshotBlocksPopulatedResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "VolumeSize": 128,
            "BlockSize": 524288,
            "ExpiryTime": 1700000000.25,
            "NextToken": "YWJjZA==",
            "Blocks": [
                {
                    "BlockIndex": 0,
                    "BlockToken": "YQ=="
                },
                {
                    "BlockIndex": 7,
                    "BlockToken": "Yg=="
                }
            ]
        },
        "body": "{\"VolumeSize\":128,\"BlockSize\":524288,\"ExpiryTime\":1700000000.25,\"NextToken\":\"YWJjZA==\",\"Blocks\":[{\"BlockIndex\":0,\"BlockToken\":\"YQ==\"},{\"BlockIndex\":7,\"BlockToken\":\"Yg==\"}]}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    },
    {
        "id": "CoverageEbsListSnapshotBlocksEmptyListResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "Blocks": [],
            "VolumeSize": 1
        },
        "body": "{\"Blocks\":[],\"VolumeSize\":1}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json"
        }
    }
])

apply com.amazonaws.ebs#GetSnapshotBlock @smithy.test#httpRequestTests([
    {
        "id": "CoverageEbsGetSnapshotBlockLabelsQueryRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "GET",
        "uri": "/snapshots/snap-01/blocks/4",
        "params": {
            "SnapshotId": "snap-01",
            "BlockIndex": 4,
            "BlockToken": "YWJjZA=="
        },
        "appliesTo": "server",
        "queryParams": [
            "blockToken=YWJjZA%3D%3D"
        ],
        "body": ""
    }
])

apply com.amazonaws.ebs#GetSnapshotBlock @smithy.test#httpResponseTests([
    {
        "id": "CoverageEbsGetSnapshotBlockStreamingBytesResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 200,
        "params": {
            "BlockData": "block-contents",
            "DataLength": 14,
            "ChecksumAlgorithm": "SHA256",
            "Checksum": "YWJjZA=="
        },
        "body": "block-contents",
        "bodyMediaType": "application/octet-stream",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/octet-stream",
            "x-amz-Data-Length": "14",
            "x-amz-Checksum-Algorithm": "SHA256",
            "x-amz-Checksum": "YWJjZA=="
        }
    }
])

apply com.amazonaws.ebs#PutSnapshotBlock @smithy.test#httpRequestTests([
    {
        "id": "CoverageEbsPutSnapshotBlockStreamingBytesRequest",
        "documentation": "Deserialize the populated fixture through the declared bindings; verify every modeled input value.",
        "protocol": "aws.protocols#restJson1",
        "method": "PUT",
        "uri": "/snapshots/snap-01/blocks/7",
        "params": {
            "SnapshotId": "snap-01",
            "BlockIndex": 7,
            "BlockData": "block-data",
            "DataLength": 10,
            "Checksum": "YWJjZA==",
            "ChecksumAlgorithm": "SHA256",
            "Progress": 100
        },
        "appliesTo": "server",
        "headers": {
            "content-type": "application/octet-stream",
            "x-amz-Data-Length": "10",
            "x-amz-Checksum": "YWJjZA==",
            "x-amz-Checksum-Algorithm": "SHA256",
            "x-amz-Progress": "100"
        },
        "body": "block-data",
        "bodyMediaType": "application/octet-stream"
    }
])

apply com.amazonaws.ebs#PutSnapshotBlock @smithy.test#httpResponseTests([
    {
        "id": "CoverageEbsPutSnapshotBlockHeadersResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 201,
        "params": {
            "Checksum": "YWJjZA==",
            "ChecksumAlgorithm": "SHA256"
        },
        "body": "{}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json",
            "x-amz-Checksum": "YWJjZA==",
            "x-amz-Checksum-Algorithm": "SHA256"
        }
    }
])

apply com.amazonaws.ebs#AccessDeniedException @smithy.test#httpResponseTests([
    {
        "id": "CoverageEbsAccessDeniedResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 403,
        "params": {
            "Message": "denied",
            "Reason": "UNAUTHORIZED_ACCOUNT"
        },
        "body": "{\"Message\":\"denied\",\"Reason\":\"UNAUTHORIZED_ACCOUNT\"}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json",
            "x-amzn-errortype": "AccessDeniedException"
        }
    }
])

apply com.amazonaws.ebs#ResourceNotFoundException @smithy.test#httpResponseTests([
    {
        "id": "CoverageEbsResourceNotFoundResponse",
        "documentation": "Serialize the specified values and compare the independently stated status, headers, and complete body.",
        "protocol": "aws.protocols#restJson1",
        "code": 404,
        "params": {
            "Message": "missing",
            "Reason": "SNAPSHOT_NOT_FOUND"
        },
        "body": "{\"Message\":\"missing\",\"Reason\":\"SNAPSHOT_NOT_FOUND\"}",
        "bodyMediaType": "application/json",
        "appliesTo": "server",
        "headers": {
            "content-type": "application/json",
            "x-amzn-errortype": "ResourceNotFoundException"
        }
    }
])
