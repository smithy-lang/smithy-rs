/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.rust.codegen.lang.RustModule
import software.amazon.smithy.rust.codegen.lang.RustWriter

class LibRsGenerator(private val libraryDocs: String, private val modules: List<RustModule>) {
    fun render(writer: RustWriter) {
        writer.setHeaderDocs(libraryDocs)
        modules.forEach { it.render(writer) }
    }
}
