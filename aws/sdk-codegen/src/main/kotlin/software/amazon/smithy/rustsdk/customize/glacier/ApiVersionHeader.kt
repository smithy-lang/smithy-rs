/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.glacier

import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq

// TODO(enableNewSmithyRuntimeCleanup): Delete this file when cleaning up middleware.

class ApiVersionHeader(
    /**
     * ApiVersion
     * This usually comes from the `version` field of the service shape and is usually a date like "2012-06-01"
     * */
    private val apiVersion: String,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.FinalizeOperation -> emptySection
        is OperationSection.OperationImplBlock -> emptySection
        is OperationSection.MutateRequest -> writable {
            rust(
                """${section.request}
                .http_mut()
                .headers_mut()
                .insert("x-amz-glacier-version", #T::HeaderValue::from_static(${apiVersion.dq()}));""",
                RuntimeType.Http,
            )
        }
        else -> emptySection
    }
}
