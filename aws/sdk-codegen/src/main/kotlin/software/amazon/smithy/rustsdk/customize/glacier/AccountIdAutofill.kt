/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.glacier

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.inputShape

// TODO(enableNewSmithyRuntimeCleanup): Delete this file when cleaning up middleware.

class AccountIdAutofill : OperationCustomization() {
    override fun mutSelf(): Boolean = true
    override fun consumesSelf(): Boolean = false
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateInput -> writable {
                rust(
                    """
                    if ${section.input}.account_id.as_deref().unwrap_or_default().is_empty() {
                        ${section.input}.account_id = Some("-".to_owned());
                    }
                    """,
                )
            }
            else -> emptySection
        }
    }

    companion object {
        fun forOperation(operation: OperationShape, model: Model): AccountIdAutofill? {
            val input = operation.inputShape(model)
            return if (input.memberNames.contains("accountId")) {
                AccountIdAutofill()
            } else {
                null
            }
        }
    }
}
