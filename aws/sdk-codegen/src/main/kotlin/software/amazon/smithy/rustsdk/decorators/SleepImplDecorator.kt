/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.decorators

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rustsdk.AwsCustomization
import software.amazon.smithy.rustsdk.AwsSection

class SleepImplDecorator : AwsCodegenDecorator {
    override val name: String = "SleepImpl"
    override val order: Byte = 0

    override fun awsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<AwsCustomization>,
    ): List<AwsCustomization> {
        return baseCustomizations + SleepImplFromSdkConfig()
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

class SleepImplFromSdkConfig : AwsCustomization() {
    override fun section(section: AwsSection): Writable = writable {
        when (section) {
            is AwsSection.FromSdkConfigForBuilder -> rust("builder.set_sleep_impl(input.sleep_impl());")
        }
    }
}
