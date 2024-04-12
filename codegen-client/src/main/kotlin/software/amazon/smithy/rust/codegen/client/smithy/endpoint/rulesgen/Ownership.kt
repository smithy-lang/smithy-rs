/*
 *  Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 *  SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen

sealed class Ownership {
    object Borrowed : Ownership() {
        override fun toString(): String {
            return "Borrowed"
        }
    }

    object Owned : Ownership() {
        override fun toString(): String {
            return "Owned"
        }
    }
}
