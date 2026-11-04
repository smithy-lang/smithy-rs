$version: "2"

// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

namespace smithy.rust.codegen.test.multiprotocol

use aws.protocols#awsJson1_0
use aws.protocols#awsJson1_1
use aws.protocols#restJson1

// Document values cannot be represented by the all-five-protocol service's XML protocol.
@awsJson1_0
@awsJson1_1
@restJson1
service MultiProtocolService {
    version: "2024-01-01"
    operations: [DocumentValues]
}
