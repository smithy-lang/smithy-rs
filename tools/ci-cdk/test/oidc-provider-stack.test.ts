/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

import { Template } from "@aws-cdk/assertions";
import * as cdk from "@aws-cdk/core";
import { OidcProviderStack } from "../lib/oidc-provider-stack";

test("it should have an OIDC provider", () => {
    const app = new cdk.App();
    const stack = new OidcProviderStack(app, "oidc-provider-stack", {});
    const template = Template.fromStack(stack);

    // Verify the OIDC provider
    template.hasResourceProperties("Custom::AWSCDKOpenIdConnectProvider", {
        ClientIDList: ["sts.amazonaws.com"],
        ThumbprintList: ["A031C46782E6E6C662C2C87C76DA9AA62CCABD8E"],
        Url: "https://token.actions.githubusercontent.com",
    });
});
