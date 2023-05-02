/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// this is a template for running feature-gate tests.
// $PLACE_HOLDER$ is replaced with relevant data types

fn main() {
    let _dt: Result<aws_smithy_types::$PLACE_HOLDER$, _> = serde_json::from_str("some_str");
}
