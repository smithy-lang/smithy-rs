/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.client.testutil

import software.amazon.smithy.rust.codegen.client.rustlang.Writable
import software.amazon.smithy.rust.codegen.client.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.RustCrate
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator

open class AddRustTestsDecorator<C : CoreCodegenContext>(
    private val testsFileName: String,
    private val testWritable: Writable,
) : RustCodegenDecorator<C> {
    override val name: String = "add tests"
    override val order: Byte = 0

    override fun extras(codegenContext: C, rustCrate: RustCrate) {
        rustCrate.withFile("tests/$testsFileName.rs") { writer ->
            writer.testWritable()
        }
    }

    // Don't allow this class to be discovered on the classpath; always return false
    override fun supportsCodegenContext(clazz: Class<out CoreCodegenContext>): Boolean = false
}
