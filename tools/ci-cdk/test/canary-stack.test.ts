/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { App } from "aws-cdk-lib";
import { Template } from "aws-cdk-lib/assertions";
import { CanaryStack } from "../lib/canary-stack";
import { METRIC_NAMESPACE } from "../lib/smithy-rs/build-canary-stack";
import { OidcProviderStack } from "../lib/oidc-provider-stack";

/** The SDK canary's own namespace, hardcoded in `canary-runner/src/run.rs`. */
const SDK_CANARY_NAMESPACE = "aws-sdk-rust-canary";

interface Statement {
    Action: string | string[];
    Effect: string;
    Resource?: string | string[];
    Condition?: Record<string, Record<string, string>>;
}

function synth(): Template {
    const app = new App();
    const oidcStack = new OidcProviderStack(app, "oidc-provider-stack", {});
    const stack = new CanaryStack(app, "smithy-rs-canary-stack", {
        lambdaExecutionRole: "smithy-rs-canary-lambda-exec-role",
        oidcProps: {
            roleId: "smithy-rs",
            roleName: "smithy-rs-canary",
            roleGithubOrg: "smithy-lang",
            roleGithubRepo: "smithy-rs",
            provider: oidcStack.githubActionsOidcProvider,
        },
    });
    return Template.fromStack(stack);
}

/** Every `PutMetricData` statement on the role this repository's CI assumes. */
function metricStatements(): Statement[] {
    const policies = Object.values(synth().findResources("AWS::IAM::Policy")).filter((p) =>
        JSON.stringify(p.Properties.Roles).includes("smithyrsoidcrole"),
    );
    expect(policies).toHaveLength(1);
    return (policies[0].Properties.PolicyDocument.Statement as Statement[]).filter((s) =>
        JSON.stringify(s.Action).includes("cloudwatch:PutMetricData"),
    );
}

/**
 * The allow has no namespace condition and every pull request's canary job assumes this role, so
 * without the deny any CI job here could trip the build canary's alarm.
 */
test("this role must not be able to write to the build canary's namespace", () => {
    const statements = metricStatements();

    const deny = statements.filter((s) => s.Effect === "Deny");
    expect(deny).toHaveLength(1);
    expect(deny[0].Condition).toEqual({
        StringEquals: { "cloudwatch:namespace": METRIC_NAMESPACE },
    });

    // Left as it was: narrowing it would change a stack `aws-sdk-rust` also deploys.
    const allow = statements.filter((s) => s.Effect === "Allow");
    expect(allow).toHaveLength(1);
    expect(allow[0].Condition).toBeUndefined();
});

/** Equal namespaces would switch off this canary's own metrics, not weaken the build canary. */
test("the two canaries must not share a namespace", () => {
    expect(METRIC_NAMESPACE).not.toEqual(SDK_CANARY_NAMESPACE);
});
