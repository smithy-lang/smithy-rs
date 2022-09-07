/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators

/**
 * Code generation mode: In some situations, codegen has different behavior for client vs. server (eg. required fields)
 */
enum class CodegenTarget {
    CLIENT, SERVER
}
