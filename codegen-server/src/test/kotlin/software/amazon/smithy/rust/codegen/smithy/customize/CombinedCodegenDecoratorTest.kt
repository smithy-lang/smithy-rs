/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customize

import io.kotest.matchers.collections.shouldContainExactly
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.RequiredCustomizations
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customizations.ServerRequiredCustomizations
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator

internal class CombinedCodegenDecoratorTest {
    private val clientDecorator: RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> = RequiredCustomizations()
    private val serverDecorator: RustCodegenDecorator<ServerProtocolGenerator, ServerCodegenContext> = ServerRequiredCustomizations()

    @Test
    fun filterClientDecorators() {
        val filteredDecorators = CombinedCodegenDecorator.filterDecorators<ClientProtocolGenerator, ClientCodegenContext>(
            listOf(clientDecorator, serverDecorator),
        ).toList()

        filteredDecorators.shouldContainExactly(clientDecorator)
    }

    @Test
    fun filterServerDecorators() {
        val filteredDecorators = CombinedCodegenDecorator.filterDecorators<ClientProtocolGenerator, ServerCodegenContext>(
            listOf(clientDecorator, serverDecorator),
        ).toList()

        filteredDecorators.shouldContainExactly(serverDecorator)
    }
}
