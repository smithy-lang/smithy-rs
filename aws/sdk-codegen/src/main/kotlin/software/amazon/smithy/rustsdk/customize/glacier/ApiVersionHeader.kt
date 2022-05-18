/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.glacier

import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.util.dq

class ApiVersionHeader(
    /**
     * ApiVersion
     * This usually comes from the `version` field of the service shape and is usually a date like "2012-06-01"
     * */
    private val apiVersion: String
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
                RuntimeType.http
            )
        }
        else -> emptySection
    }
}
