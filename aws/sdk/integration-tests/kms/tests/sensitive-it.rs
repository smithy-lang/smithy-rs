/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use kms::output::GenerateRandomOutput;
use kms::Blob;
#[test]
fn validate_sensitive_trait() {
    let output = GenerateRandomOutput::builder().plaintext(Blob::new("some output")).build();
    assert_eq!(format!("{:?}", output), "GenerateRandomOutput { plaintext: \"*** Sensitive Data Redacted ***\" }");
}
