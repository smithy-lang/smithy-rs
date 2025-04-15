/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk.customize.dynamodb

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable

/**
 * This decorator is for DynamoDB to mark certain APIs as deprecated that were never intended
 * to be made public.
 *
 * Setters for the account ID were introduced in smithy-rs#3792, but they should not be configurable
 * externally.
 */
class DynamoDbDecorator : ClientCodegenDecorator {
    override val name: String = "DynamoDb"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations +
            object : ConfigCustomization() {
                override fun section(section: ServiceConfig): Writable {
                    return when (section) {
                        ServiceConfig.BuilderImpl ->
                            writable {
                                rust(
                                    """

                                    ##[deprecated(note = "This method wasn't intended to be public, and is no longer functional. Do not use.")]
                                    /// The AWS AccountId used for the request.
                                    pub fn account_id(mut self, _account_id: impl Into<::std::string::String>) -> Self {
                                        self
                                    }

                                    ##[deprecated(note = "This method wasn't intended to be public, and is no longer functional. Do not use.")]
                                    /// The AWS AccountId used for the request.
                                    pub fn set_account_id(&mut self, _account_id: ::std::option::Option<::std::string::String>) -> &mut Self {
                                        self
                                    }
                                    """,
                                )
                            }

                        else -> emptySection
                    }
                }
            }
    }
}
