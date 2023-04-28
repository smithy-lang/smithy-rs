/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

fn main() {
    let _dt: Result<ReplaceDataType, _> = serde_json::from_str("some_str");
}
