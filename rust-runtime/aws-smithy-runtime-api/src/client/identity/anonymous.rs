/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::client::identity::{Identity, IdentityResolver};
use crate::client::orchestrator::Future;
use crate::config_bag::ConfigBag;

#[derive(Debug)]
pub struct AnonymousIdentity;

impl AnonymousIdentity {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug)]
pub struct AnonymousIdentityResolver;

impl AnonymousIdentityResolver {
    pub fn new() -> Self {
        AnonymousIdentityResolver
    }
}

impl IdentityResolver for AnonymousIdentityResolver {
    fn resolve_identity(&self, _: &ConfigBag) -> Future<Identity> {
        Future::ready(Ok(Identity::new(AnonymousIdentity::new(), None)))
    }
}
