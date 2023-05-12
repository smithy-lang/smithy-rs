/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.NamingObstacleCourseTestModels
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class NamingObstacleCourseTest {
    @Test
    fun `test Rust prelude operation names compile`() {
        serverIntegrationTest(NamingObstacleCourseTestModels.rustPreludeOperationsModel()) { _, _ -> }
    }

    @Test
    fun `test Rust prelude structure names compile`() {
        serverIntegrationTest(NamingObstacleCourseTestModels.rustPreludeStructsModel()) { _, _ -> }
    }

    @Test
    fun `test Rust prelude enum names compile`() {
        serverIntegrationTest(NamingObstacleCourseTestModels.rustPreludeEnumsModel()) { _, _ -> }
    }

    @Test
    fun `test Rust prelude enum variant names compile`() {
        serverIntegrationTest(NamingObstacleCourseTestModels.rustPreludeEnumVariantsModel()) { _, _ -> }
    }
}
