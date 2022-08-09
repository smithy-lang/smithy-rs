/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import org.junit.jupiter.api.Assertions
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.CsvSource

class VersionTest {
    @ParameterizedTest()
    @CsvSource(
        "'0.47.0-0198d26096eb1af510ce24766c921ffc5e4c191e','0.47.0-0198d26096eb1af510ce24766c921ffc5e4c191e','0.47.0'",
        "'0.0.0','0.0.0',''",
        "'','',''",
    )
    fun `parses version`(
        content: String,
        fullVersion: String,
        crateVersion: String,
    ) {
        Assertions.assertEquals(fullVersion, Version(content).fullVersion())
        Assertions.assertEquals(crateVersion, Version(content).crateVersion())
    }
}
