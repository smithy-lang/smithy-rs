/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::provider_config::ProviderConfig;
use std::borrow::Cow;
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
enum PropertySource<'a> {
    Environment { name: &'a str },
    Profile { name: &'a str, key: &'a str },
}

#[derive(Debug)]
pub(crate) struct Context<'a> {
    source: PropertySource<'a>,
}

impl Display for Context<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.source {
            PropertySource::Environment { name } => write!(f, "environment variable `{}`", name),
            PropertySource::Profile { name, key } => {
                write!(f, "profile `{}`, key: `{}`", name, key)
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct PropertyResolutionError<E = Box<dyn Error>> {
    property_source: String,
    pub(crate) err: E,
}

impl<E: Display> Display for PropertyResolutionError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}. source: {}", self.err, self.property_source)
    }
}

impl<E: Error> Error for PropertyResolutionError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.err.source()
    }
}

/// Standard properties simplify code that reads properties from the environment and AWS Profile
///
/// `StandardProperty` will first look in the environment, then the AWS profile. They track the
/// provenance of properties so that unified validation errors can be created.
#[derive(Default)]
pub(crate) struct StandardProperty {
    environment_variable: Option<Cow<'static, str>>,
    profile_key: Option<Cow<'static, str>>,
}

impl StandardProperty {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn env(mut self, key: &'static str) -> Self {
        self.environment_variable = Some(Cow::Borrowed(key));
        self
    }

    pub(crate) fn profile(mut self, key: &'static str) -> Self {
        self.profile_key = Some(Cow::Borrowed(key));
        self
    }

    pub(crate) async fn validate<T, E: Error + Send + Sync + 'static>(
        self,
        provider_config: &ProviderConfig,
        validator: impl Fn(&str) -> Result<T, E>,
    ) -> Result<Option<T>, PropertyResolutionError<E>> {
        let value = self.load(provider_config).await;
        value
            .map(|(v, ctx)| {
                validator(v.as_ref()).map_err(|err| PropertyResolutionError {
                    property_source: format!("{}", ctx),
                    err,
                })
            })
            .transpose()
    }

    pub(crate) async fn load<'a>(
        &'a self,
        provider_config: &'a ProviderConfig,
    ) -> Option<(Cow<'a, str>, Context<'a>)> {
        if let Some(env_var) = self.environment_variable.as_ref() {
            if let Ok(value) = provider_config.env().get(env_var) {
                return Some((
                    Cow::Owned(value),
                    Context {
                        source: PropertySource::Environment { name: env_var },
                    },
                ));
            }
        }
        if let Some(profile_key) = self.profile_key.as_ref() {
            let profile = provider_config.profile().await?;

            if let Some(value) = profile.get(profile_key) {
                return Some((
                    Cow::Borrowed(value),
                    Context {
                        source: PropertySource::Profile {
                            name: profile.selected_profile(),
                            key: profile_key,
                        },
                    },
                ));
            }
        }

        return None;
    }
}
