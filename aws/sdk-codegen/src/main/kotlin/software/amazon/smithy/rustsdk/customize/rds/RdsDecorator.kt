/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk.customize.rds

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rustsdk.AwsCargoDependency
import software.amazon.smithy.rustsdk.InlineAwsDependency

class RdsDecorator : ClientCodegenDecorator {
    override val name: String = "RDS"
    override val order: Byte = 0

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        val rc = codegenContext.runtimeConfig

        rustCrate.lib {
            // We should have a better way of including an inline dependency.
            rust(
                "// include #T;",
                RuntimeType.forInlineDependency(
                    InlineAwsDependency.forRustFileAs(
                        "rds_auth_token",
                        "auth_token",
                        Visibility.PUBLIC,
                        AwsCargoDependency.awsSigv4(rc),
                        CargoDependency.smithyRuntimeApiClient(rc),
                        CargoDependency.smithyAsync(rc).toDevDependency().withFeature("test-util"),
                        CargoDependency.Url,
                    ),
                ),
            )
        }
    }
}
