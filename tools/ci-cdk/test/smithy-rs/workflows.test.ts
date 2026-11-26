/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { existsSync } from "fs";
import { join } from "path";

import { LEG_WORKFLOW, ORCHESTRATOR_WORKFLOWS } from "../../lib/smithy-rs/build-canary-stack";
import {
    CANARY_WORKFLOW,
    MANUAL_CANARY_WORKFLOW,
    PULL_REQUEST_BOT_WORKFLOW,
} from "../../lib/smithy-rs/workflows";

const WORKFLOW_DIR = join(__dirname, "../../../../.github/workflows");

/** Every workflow filename a trust policy in this repository pins. */
const PINNED = [
    LEG_WORKFLOW,
    ...ORCHESTRATOR_WORKFLOWS,
    CANARY_WORKFLOW,
    MANUAL_CANARY_WORKFLOW,
    PULL_REQUEST_BOT_WORKFLOW,
];

/**
 * A pinned name is a string TypeScript cannot check against the directory it refers to.
 *
 * Getting one wrong produces a role nothing can assume, and because a trust policy only changes
 * when its stack is deployed, the failure lands on whoever next deploys rather than on whoever
 * renamed the file. The symptom is an opaque credentials error in a job that has nothing to do with
 * the rename.
 *
 * Only the workflows that already exist are checked here. The build canary's three arrive with the
 * canary itself, and the invariants in its own crate load each of them by name, so a rename there
 * fails those tests rather than going unnoticed.
 */
test("every pinned workflow file that exists yet is where the roles say", () => {
    expect(existsSync(WORKFLOW_DIR)).toBe(true);
    for (const workflow of [CANARY_WORKFLOW, MANUAL_CANARY_WORKFLOW, PULL_REQUEST_BOT_WORKFLOW]) {
        expect({ workflow, exists: existsSync(join(WORKFLOW_DIR, workflow)) }).toEqual({
            workflow,
            exists: true,
        });
    }
});

/** Two roles pinning one file would be two roles a single token can assume. */
test("no workflow is pinned by more than one role", () => {
    expect(new Set(PINNED).size).toBe(PINNED.length);
});
