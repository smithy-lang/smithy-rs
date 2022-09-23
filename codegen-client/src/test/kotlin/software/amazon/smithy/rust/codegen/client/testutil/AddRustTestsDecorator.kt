/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.client.testutil

import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

open class AddRustTestsDecorator<T, C : CodegenContext>(
    private val testsFileName: String,
    private val testWritable: Writable,
) : RustCodegenDecorator<T, C> {
    override val name: String = "add tests"
    override val order: Byte = 0

    override fun extras(codegenContext: C, rustCrate: RustCrate) {
        rustCrate.withFile("tests/$testsFileName.rs") { writer ->
            writer.testWritable()
        }
    }

    // Don't allow this class to be discovered on the classpath; always return false
    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean = false
}
