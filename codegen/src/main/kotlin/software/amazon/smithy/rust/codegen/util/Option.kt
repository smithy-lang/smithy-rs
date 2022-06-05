/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.util

import java.util.Optional

fun <T> Optional<T>.orNull(): T? = this.orElse(null)
