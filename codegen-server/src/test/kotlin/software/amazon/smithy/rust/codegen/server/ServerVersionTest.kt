/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test

internal class ServerVersionTest {
    @Test
    fun `parses artifact version separately from commit`() {
        val version = ServerVersion.parse("""{"codegenVersion":"0.1.32-SNAPSHOT","gitHash":"abc123"}""")
        version.codegenVersion shouldBe "0.1.32-SNAPSHOT"
        version.gitHash shouldBe "abc123"
    }
}
