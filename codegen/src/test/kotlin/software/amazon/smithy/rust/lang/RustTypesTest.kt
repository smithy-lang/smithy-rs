/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.lang

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.lang.RustType
import software.amazon.smithy.rust.codegen.lang.render

class RustTypesTest {
    @Test
    fun `types render properly`() {
        val type = RustType.Box(RustType.Option(RustType.Reference("a", RustType.Vec(RustType.String))))
        type.render() shouldBe "Box<Option<&'a Vec<String>>>"
    }
}
