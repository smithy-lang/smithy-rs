/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.raw
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig

class CrateLicenseDecorator : RustCodegenDecorator {
    override val name: String = "CrateLicense"

    override val order: Byte = 0

    override fun extras(rustSettings: RustSettings, protocolConfig: ProtocolConfig, rustCrate: RustCrate) {
        rustCrate.withFile("LICENSE") {
            val license = this::class.java.getResource("/LICENSE").readText()
            it.raw(license)
        }
    }
}
