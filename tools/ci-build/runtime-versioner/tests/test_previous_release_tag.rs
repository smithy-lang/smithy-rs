/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use test_common::TestBase;

#[test]
fn previous_release_tag() {
    let test_base = TestBase::new("all_smithy_rs_head");
    assert_eq!(
        "release-2023-10-01\n",
        test_base
            .run_versioner(&["previous-release-tag"], false)
            .stdout
    );

    test_base.change_branch("change_crate_with_bump");
    assert_eq!(
        "release-2023-10-02\n",
        test_base
            .run_versioner(&["previous-release-tag"], false)
            .stdout
    );
}
