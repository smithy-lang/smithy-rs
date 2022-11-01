/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedSectionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section

sealed class AwsSection(name: String) : Section(name) {
    abstract val customizations: List<AwsCustomization>

    /**
     * Write custom code into the `impl From<&aws_types::sdk_config::SdkConfig> for Builder`.
     * This `From` impl takes a reference; Don't forget to clone non-`Copy` types.
     */
    data class FromSdkConfigForBuilder(override val customizations: List<AwsCustomization>) :
        AwsSection("FromSdkConfig")
}

abstract class AwsCustomization : NamedSectionGenerator<AwsSection>()
