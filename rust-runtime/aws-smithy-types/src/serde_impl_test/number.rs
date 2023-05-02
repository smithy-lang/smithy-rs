/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::*;

/// Tests whether de-serialization feature for number is properly feature gated
#[test]
fn feature_gate_test_for_number_deserialization() {
    // create files
    let cargo_project_path = create_cargo_dir("Number", Target::De);
    de_test(&cargo_project_path);
}

/// Tests whether serialization feature for number is properly feature gated
#[test]
fn feature_gate_test_for_number_serialization() {
    // create files
    let cargo_project_path = create_cargo_dir("Number", Target::Ser);
    ser_test(&cargo_project_path);
}
