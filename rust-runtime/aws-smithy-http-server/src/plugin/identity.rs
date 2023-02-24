/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::operation::Operation;

use super::Plugin;

/// A [`Plugin`] that maps an `input` [`Operation`] to itself.
pub struct IdentityPlugin;

impl<P, Op, S, L> Plugin<P, Op, S, L> for IdentityPlugin {
    type Service = S;
    type Layer = L;

    fn map(&self, input: Operation<S, L>) -> Operation<S, L> {
        input
    }
}
