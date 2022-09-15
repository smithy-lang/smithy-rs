/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.rustlang.raw
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.RustCrate
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator

class CrateLicenseDecorator : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "CrateLicense"

    override val order: Byte = 0

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        rustCrate.withFile("LICENSE") {
            val license = this::class.java.getResource("/LICENSE").readText()
            it.raw(license)
        }
    }

    override fun supportsCodegenContext(clazz: Class<out CoreCodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}
