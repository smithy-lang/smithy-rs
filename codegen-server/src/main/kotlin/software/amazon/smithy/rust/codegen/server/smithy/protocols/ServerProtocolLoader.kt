/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.rust.codegen.client.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.protocols.AwsJsonVersion
import software.amazon.smithy.rust.codegen.client.smithy.protocols.ProtocolLoader
import software.amazon.smithy.rust.codegen.client.smithy.protocols.ProtocolMap

class ServerProtocolLoader(supportedProtocols: ProtocolMap<ServerCodegenContext>) :
    ProtocolLoader<ServerCodegenContext>(supportedProtocols) {

    companion object {
        val DefaultProtocols = mapOf(
            RestJson1Trait.ID to ServerRestJsonFactory(),
            RestXmlTrait.ID to ServerRestXmlFactory(),
            AwsJson1_0Trait.ID to ServerAwsJsonFactory(AwsJsonVersion.Json10),
            AwsJson1_1Trait.ID to ServerAwsJsonFactory(AwsJsonVersion.Json11),
        )
    }
}
