/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.writable

class ServiceConfigDecorator : ClientCodegenDecorator {
    override val name: String = "ServiceConfigGenerator"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations + SharedConfigDocsCustomization()
}

class SharedConfigDocsCustomization : ConfigCustomization() {
    override fun section(section: ServiceConfig): Writable {
        return if (section is ServiceConfig.ConfigStructAdditionalDocs) {
            writable {
                docs(
                    """Service configuration allows for customization of endpoints, region, credentials providers,
                    and retry configuration. Generally, it is constructed automatically for you from a shared
                    configuration loaded by the `aws-config` crate. For example:

                    ```ignore
                    // Load a shared config from the environment
                    let shared_config = aws_config::from_env().load().await;
                    // The client constructor automatically converts the shared config into the service config
                    let client = Client::new(&shared_config);
                    ```

                    The service config can also be constructed manually using its builder.
                    """,
                )
            }
        } else {
            emptySection
        }
    }
}
