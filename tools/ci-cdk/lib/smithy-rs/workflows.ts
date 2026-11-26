/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * The workflow files this repository's trust policies pin, by `job_workflow_ref`.
 *
 * A role conditioned on the repository alone is assumable by every job in it that holds
 * `id-token: write`. Several of those jobs check out and build code nobody has reviewed -- a pull
 * request's, or a crate published to crates.io -- so which workflow a job is defined in is part of
 * what each role should be deciding on.
 *
 * Collected here because these are filenames in `.github/workflows/`, which TypeScript cannot
 * check. A rename would leave a role nothing can assume, and the symptom lands on whoever next
 * deploys rather than on whoever renamed it. `every pinned workflow file exists` in the tests is
 * what closes that.
 */

/** Runs the pull request canary. A reusable workflow, called by the four `ci-*.yml` entry points. */
export const CANARY_WORKFLOW = "ci.yml";

/**
 * The canary's other entry point: dispatched by a maintainer, and called by `fork-canary.yml` for a
 * labelled fork pull request.
 */
export const MANUAL_CANARY_WORKFLOW = "manual-canary.yml";

/**
 * Posts the codegen diff and doc preview. A reusable workflow, called by `ci-pr.yml` on every pull
 * request and by `manual-pull-request-bot.yml` by hand.
 */
export const PULL_REQUEST_BOT_WORKFLOW = "pull-request-bot.yml";
