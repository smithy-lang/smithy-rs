/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

rootProject.name = "smithy-rs"

include(":codegen-core")
include(":codegen-client")
include(":codegen-client-test")
include(":codegen-server")
include(":codegen-serde")
include(":codegen-server:python")
include(":codegen-server:typescript")
include(":codegen-server-test")
include(":codegen-server-test:python")
include(":codegen-server-test:typescript")
include(":rust-runtime")
include(":aws:rust-runtime")
include(":aws:sdk")
include(":aws:sdk-adhoc-test")
include(":aws:sdk-codegen")
include(":fuzzgen")
