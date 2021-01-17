/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

class RegionConfig : ConfigCustomization() {
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rust("pub region: String,")
            is ServiceConfig.ConfigImpl -> emptySection
            is ServiceConfig.BuilderStruct ->
                rust("region: Option<String>,")
            ServiceConfig.BuilderImpl ->
                rust(
                    """
            pub fn region(mut self, region: impl ToString) -> Self {
                self.region = Some(region.to_string());
                self
            }
            """,
                )
            ServiceConfig.BuilderPreamble -> rust(
                // TODO: design a config that enables resolving the default region chain
                // clone because the region is also used
                """let region = self.region.unwrap_or_else(|| "us-east-1".to_string());""",
            )
            ServiceConfig.BuilderBuild -> rust(
                """region: region.clone(),""",
            )
        }
    }
}
