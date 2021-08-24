/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

pub mod middleware;
use aws_types::credential::ProvideCredentials;
pub use aws_types::Credentials;
use smithy_http::property_bag::PropertyBag;
use std::sync::Arc;

pub type CredentialsProvider = Arc<dyn ProvideCredentials>;

pub fn set_provider(bag: &mut PropertyBag, provider: CredentialsProvider) {
    bag.insert(provider);
}
