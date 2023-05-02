/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::*;

/// Tests whether de-serialization feature for datetime is properly feature gated
#[test]
fn feature_gate_test_for_datetime_deserialization() {
    // create files
    let cargo_project_path = create_cargo_dir("DateTime", Target::De);
    de_test(&cargo_project_path);
}

/// Tests whether serialization feature for datetime is properly feature gated
#[test]
fn feature_gate_test_for_datetime_serialization() {
    // create files
    let cargo_project_path = create_cargo_dir("DateTime", Target::Ser);
    ser_test(&cargo_project_path);
}
