/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::provider_config::ProviderConfig;
use aws_runtime::env_config::EnvConfigValue;
use aws_smithy_runtime_api::client::auth::{AuthSchemeId, AuthSchemePreference};
use aws_smithy_types::error::display::DisplayErrorContext;
use std::borrow::Cow;
use std::fmt;

mod env {
    pub(super) const AWS_AUTH_SCHEME_PREFERENCE: &str = "AWS_AUTH_SCHEME_PREFERENCE";
}

mod profile_key {
    pub(super) const AWS_AUTH_SCHEME_PREFERENCE: &str = "auth_scheme_preference";
}

/// Load the value for the auth scheme preference, a list of ordered `AuthSchemeId`s
///
/// This checks the following sources:
/// 1. The environment variable `AWS_AUTH_SCHEME_PREFERENCE=scheme1,scheme2,scheme3`
/// 2. The profile key `auth_scheme_preference=scheme1,scheme2,scheme3`
///
/// Space and tab characters between names will be ignored.
/// A scheme name can just be a `sigv4` or a fully qualified trait name `aws.auth#sigv4`.
pub(crate) async fn auth_scheme_preference_provider(
    provider_config: &ProviderConfig,
) -> Option<AuthSchemePreference> {
    let env = provider_config.env();
    let profiles = provider_config.profile().await;

    EnvConfigValue::new()
        .env(env::AWS_AUTH_SCHEME_PREFERENCE)
        .profile(profile_key::AWS_AUTH_SCHEME_PREFERENCE)
        .validate(&env, profiles, parse_auth_scheme_names)
        .map_err(|err| tracing::warn!(err = %DisplayErrorContext(&err), "invalid value for `AuthSchemePreference`"))
        .unwrap_or(None)
}

fn parse_auth_scheme_names(csv: &str) -> Result<AuthSchemePreference, InvalidAuthSchemeNamesCsv> {
    csv.split(',')
        .map(|s| {
            let trimmed = s.trim().replace(|c| c == ' ' || c == '\t', "");
            if trimmed.is_empty() {
                return Err(InvalidAuthSchemeNamesCsv {
                    value: format!("Empty name found in `{csv}`."),
                });
            }
            let scheme_name = trimmed.split('#').last().unwrap_or(&trimmed).to_owned();
            Ok(AuthSchemeId::from(Cow::Owned(scheme_name)))
        })
        .collect()
}

#[derive(Debug)]
pub(crate) struct InvalidAuthSchemeNamesCsv {
    value: String,
}

impl fmt::Display for InvalidAuthSchemeNamesCsv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Not a valid comma-separated auth scheme names: {}",
            self.value
        )
    }
}

impl std::error::Error for InvalidAuthSchemeNamesCsv {}
