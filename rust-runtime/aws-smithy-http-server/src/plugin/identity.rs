/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{HttpMarker, ModelMarker, Plugin};

/// A [`Plugin`] that maps a service to itself.
#[derive(Debug)]
pub struct IdentityPlugin;

impl<Ser, Op, S> Plugin<Ser, Op, S> for IdentityPlugin {
    type Output = S;

    fn apply(&self, svc: S) -> S {
        svc
    }
}

impl ModelMarker for IdentityPlugin {}
impl HttpMarker for IdentityPlugin {}
