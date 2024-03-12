/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::env_config::{EnvConfigError, EnvConfigValue};
use crate::profile::profile_set::ProfileSet;
use aws_types::os_shim_internal::Env;
use aws_types::service_config::ServiceConfigKey;
use std::error::Error;

pub mod error;
mod normalize;
pub mod parse;
pub mod profile_file;
pub mod profile_set;
pub mod section;
pub mod source;

/// Given a key, access to the environment, and a validator, return a config value if one was set.
pub async fn get_service_env_config<'a, T, E>(
    key: ServiceConfigKey<'a>,
    env: &'a Env,
    profiles: Option<&'a ProfileSet>,
    validator: impl Fn(&str) -> Result<T, E>,
) -> Result<Option<T>, EnvConfigError<E>>
where
    E: Error + Send + Sync + 'static,
{
    EnvConfigValue::default()
        .env(key.env())
        .profile(key.profile())
        .service_id(key.service_id())
        .validate(env, profiles, validator)
}
