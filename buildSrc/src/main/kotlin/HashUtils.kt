/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import java.io.File
import java.security.MessageDigest

fun ByteArray.toHex() = joinToString(separator = "") { byte -> "%02x".format(byte) }

fun getChecksumForFile(file: File, digest: MessageDigest = MessageDigest.getInstance("SHA-256")): String =
    digest.digest(file.readText().toByteArray()).toHex()
