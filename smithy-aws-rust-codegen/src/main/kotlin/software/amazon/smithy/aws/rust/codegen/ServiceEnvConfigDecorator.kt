/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.aws.rust.codegen

import software.amazon.smithy.rust.codegen.client.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.ClientRustModule
import software.amazon.smithy.rust.codegen.client.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.RustCrate
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.sdkId
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

class ServiceEnvConfigDecorator : ClientCodegenDecorator {
    override val name: String = "ServiceEnvConfigDecorator"
    override val order: Byte = 10

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        val rc = codegenContext.runtimeConfig
        val serviceId = codegenContext.serviceShape.sdkId().toSnakeCase().dq()
        rustCrate.withModule(ClientRustModule.config) {
            Attribute.AllowDeadCode.render(this)
            rustTemplate(
                """
                fn service_config_key<'a>(
                    env: &'a str,
                    profile: &'a str,
                ) -> aws_types::service_config::ServiceConfigKey<'a> {
                    #{ServiceConfigKey}::builder()
                        .service_id($serviceId)
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
