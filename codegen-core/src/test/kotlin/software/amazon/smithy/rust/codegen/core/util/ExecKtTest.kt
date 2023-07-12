/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.util

import io.kotest.assertions.throwables.shouldThrow
import org.junit.jupiter.api.Test

internal class ExecKtTest {
    @Test
    fun `missing command throws CommandFailed`() {
        shouldThrow<CommandError> {
            "notaprogram run".runCommand()
        }
    }
}
