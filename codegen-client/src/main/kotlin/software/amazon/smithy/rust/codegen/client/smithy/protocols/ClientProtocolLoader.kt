/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import software.amazon.smithy.aws.traits.protocols.AwsJson1_0Trait
import software.amazon.smithy.aws.traits.protocols.AwsJson1_1Trait
import software.amazon.smithy.aws.traits.protocols.AwsQueryTrait
import software.amazon.smithy.aws.traits.protocols.Ec2QueryTrait
import software.amazon.smithy.aws.traits.protocols.RestJson1Trait
import software.amazon.smithy.aws.traits.protocols.RestXmlTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolLoader
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap

class ClientProtocolLoader(supportedProtocols: ProtocolMap<ClientProtocolGenerator, ClientCodegenContext>) :
    ProtocolLoader<ClientProtocolGenerator, ClientCodegenContext>(supportedProtocols) {

    companion object {
        val DefaultProtocols = mapOf(
            AwsJson1_0Trait.ID to AwsJsonFactory(AwsJsonVersion.Json10),
            AwsJson1_1Trait.ID to AwsJsonFactory(AwsJsonVersion.Json11),
            AwsQueryTrait.ID to AwsQueryFactory(),
            Ec2QueryTrait.ID to Ec2QueryFactory(),
            RestJson1Trait.ID to RestJsonFactory(),
            RestXmlTrait.ID to RestXmlFactory(),
        )
        val Default = ClientProtocolLoader(DefaultProtocols)
    }
}
