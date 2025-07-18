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
// NOTE: we rename the aws/rust-runtime project so that it ends up with different GAV
// coordinates otherwise it will conflict with the generic runtime which has coordinates
// `software.amazon.smithy.rust:rust-runtime`.
project(":aws:rust-runtime").name = "aws-rust-runtime"
include(":aws:sdk")
include(":aws:sdk-adhoc-test")
include(":aws:sdk-codegen")
include(":fuzzgen")
