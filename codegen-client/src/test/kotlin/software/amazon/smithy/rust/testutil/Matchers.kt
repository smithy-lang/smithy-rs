/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.testutil

import io.kotest.matchers.shouldBe

fun <T> String.shouldMatchResource(clazz: Class<T>, resourceName: String) {
    val resource = clazz.getResource(resourceName).readText()
    this.trim().shouldBe(resource.trim())
}
