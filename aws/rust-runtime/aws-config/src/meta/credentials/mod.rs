/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

mod chain;
pub use chain::CredentialsProviderChain;

mod credential_fn;
pub use credential_fn::async_provide_credentials_fn;

mod lazy_caching;
pub use lazy_caching::LazyCachingCredentialsProvider;

// pub mod credential_fn;
// pub mod lazy_caching;

// mod cache;
// mod time;
