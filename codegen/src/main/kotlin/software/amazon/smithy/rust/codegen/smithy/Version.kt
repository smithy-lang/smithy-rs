/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.CodegenException

class Version {
    companion object {
        // generated as part of the build, see codegen/build.gradle.kts
        private const val CRATE_VERSION_FILENAME = "runtime-crate-version.txt"

        fun crateVersion(): String {
            return object {}.javaClass.getResource(CRATE_VERSION_FILENAME)?.readText()
                ?: throw CodegenException("$CRATE_VERSION_FILENAME does not exist")
        }
    }
}
