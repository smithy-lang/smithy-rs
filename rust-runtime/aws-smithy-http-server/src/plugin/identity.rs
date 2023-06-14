/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::Plugin;

/// A [`Plugin`] that maps a service to itself.
pub struct IdentityPlugin;

impl<P, Op, S> Plugin<P, Op, S> for IdentityPlugin {
    type Service = S;

    fn apply(&self, svc: S) -> S {
        svc
    }
}
