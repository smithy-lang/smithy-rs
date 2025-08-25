/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

class ServiceEnvConfigDecorator : ClientCodegenDecorator {
    override val name: String = "ServiceEnvConfigDecorator"
    override val order: Byte = 10

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        val rc = codegenContext.runtimeConfig
        rustCrate.withModule(ClientRustModule.config) {
            Attribute.AllowDeadCode.render(this)
            rustTemplate(
                """
                fn service_config_key<'a>(
                    service_id: &'a str,
                    env: &'a str,
                    profile: &'a str,
                ) -> aws_types::service_config::ServiceConfigKey<'a> {
                    #{ServiceConfigKey}::builder()
                        .service_id(service_id)
                        .env(env)
                        .profile(profile)
                        .build()
                        .expect("all field sets explicitly, can't fail")
                }
                """,
                "ServiceConfigKey" to AwsRuntimeType.awsTypes(rc).resolve("service_config::ServiceConfigKey"),
            )
        }
    }
}
