/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.rust.codegen.lang.RustWriter

class LibRsGenerator(private val modules: List<String>, private val writer: RustWriter) {
    fun render() {
        modules.forEach { writer.write("pub mod $it;") }
    }
}
