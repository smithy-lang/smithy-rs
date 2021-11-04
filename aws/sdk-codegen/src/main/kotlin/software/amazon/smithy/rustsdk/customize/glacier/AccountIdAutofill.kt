/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk.customize.glacier

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.util.inputShape

class AccountIdAutofill() : OperationCustomization() {
    override fun mutSelf(): Boolean = true

    // this is a bit of a hack, but there currently isn't a good way to pass the information up the chain into the
    // fluent builder. I think we what we actually want is to write this information into the symbol metadata, but TODO.
    override fun consumesSelf(): Boolean = true
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateInput -> writable {
                rust(
                    """
                    if ${section.input}.account_id.as_deref().unwrap_or_default().is_empty() {
                        ${section.input}.account_id = Some("-".to_owned());
                    }
                    """
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
            } else null
        }
    }
}
