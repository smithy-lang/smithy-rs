/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.raw
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

class CrateLicenseDecorator : ClientCodegenDecorator {
    override val name: String = "CrateLicense"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        rustCrate.withFile("LICENSE") {
            val license = this::class.java.getResource("/LICENSE").readText()
            raw(license)
        }
    }
}
