/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

pub mod middleware;
pub mod provider;

use aws_types::credentials::SharedCredentialsProvider;
use smithy_http::property_bag::PropertyBag;

pub fn set_provider(bag: &mut PropertyBag, provider: SharedCredentialsProvider) {
    bag.insert(provider);
}
