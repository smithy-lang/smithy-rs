/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

/**
 * The artifact type for whom we are generating the structure.
 */
enum class CodegenTarget {
    CLIENT, SERVER
}
