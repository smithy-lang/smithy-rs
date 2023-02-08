/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.dq

class OperationBuildError(private val runtimeConfig: RuntimeConfig) {
    fun missingField(field: String, details: String) = writable {
        rust("#T::missing_field(${field.dq()}, ${details.dq()})", runtimeConfig.operationBuildError())
    }
    fun invalidField(field: String, details: String) = invalidField(field) { rust(details.dq()) }
    fun invalidField(field: String, details: Writable) = writable {
        rustTemplate(
            "#{error}::invalid_field(${field.dq()}, #{details:W})",
            "error" to runtimeConfig.operationBuildError(),
            "details" to details,
        )
    }
}

fun RuntimeConfig.operationBuildError() = RuntimeType.operationModule(this).resolve("error::BuildError")
