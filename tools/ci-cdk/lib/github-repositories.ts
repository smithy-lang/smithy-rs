/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * The GitHub repositories whose workflows assume a role deployed from here, with their ids.
 *
 * The ids are what the trust policies authorise on, because organisation and repository names are
 * mutable and a freed name can be claimed by another account. They survive a rename. A transfer is
 * undocumented either way, so re-read them after one.
 *
 * Read them with `gh api repos/ORG/REPO --jq '{repository_id: .id, owner_id: .owner.id}'`. A wrong
 * value here produces a role nobody can assume, which surfaces as an opaque permissions error in
 * CI rather than at deploy time.
 */
export const SMITHY_RS = {
    githubOrg: "smithy-lang",
    githubRepo: "smithy-rs",
    githubOrgId: "133827778",
    githubRepoId: "308027791",
} as const;

/** The SDK repository, whose canary stack is deployed from `bin/aws-sdk-rust`. */
export const AWS_SDK_RUST = {
    githubOrg: "awslabs",
    githubRepo: "aws-sdk-rust",
    githubOrgId: "3299148",
    githubRepoId: "357284448",
} as const;
