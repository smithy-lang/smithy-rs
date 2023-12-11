/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/** Returns a symbol for a type re-exported into crate::config
 *  Although it is not always possible to use this, this is the preferred method for using types in config customizations
 *  and ensures that your type will be re-exported if it is used.
 */
fun configReexport(type: RuntimeType): RuntimeType = RuntimeType.forInlineFun(type.name, module = ClientRustModule.config) {
    rustTemplate("pub use #{type};", "type" to type)
}
