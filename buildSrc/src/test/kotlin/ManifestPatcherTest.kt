/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test

class ManifestPatcherTest {
    @Test
    fun `it should rewrite crate versions`() {
        val version = "0.10.0"
        assertEquals("", rewriteCrateVersion("", version))
        assertEquals("version = \"0.10.0\"", rewriteCrateVersion("version=\"0.0.0-smithy-rs-head\"", version))
        assertEquals("version = \"0.10.0\"", rewriteCrateVersion(" version = \"0.0.0-smithy-rs-head\"", version))
        assertEquals("version = \"1.5.0\"", rewriteCrateVersion("version = \"1.5.0\"", version))
    }
}
