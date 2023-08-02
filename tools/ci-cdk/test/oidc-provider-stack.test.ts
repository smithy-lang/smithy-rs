/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { App } from "aws-cdk-lib";
import { Template } from "aws-cdk-lib/assertions";
import { GITHUB_CERTIFICATE_THUMBPRINTS, OidcProviderStack } from "../lib/oidc-provider-stack";

test("it should have an OIDC provider", () => {
    const app = new App();
    const stack = new OidcProviderStack(app, "oidc-provider-stack", {});
    const template = Template.fromStack(stack);

    // Verify the OIDC provider
    template.hasResourceProperties("Custom::AWSCDKOpenIdConnectProvider", {
        ClientIDList: ["sts.amazonaws.com"],
        ThumbprintList: GITHUB_CERTIFICATE_THUMBPRINTS,
        Url: "https://token.actions.githubusercontent.com",
    });
});
