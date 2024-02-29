/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::profile::{ProfileSet, PropertiesKey};
use crate::provider_config::ProviderConfig;
use std::borrow::Cow;
use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub(crate) enum PropertySource<'a> {
    Environment {
        name: &'a str,
        service_specific: bool,
    },
    Profile {
        name: &'a str,
        key: &'a str,
        service_specific: bool,
    },
}

impl Display for PropertySource<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PropertySource::Environment {
                name,
                service_specific,
            } => {
                let service_specific = service_specific
                    .then_some("service-specific ")
                    .unwrap_or_default();
                write!(f, "{service_specific}environment variable `{name}`")
            }
            PropertySource::Profile {
                name,
                key,
                service_specific,
            } => {
                let service_specific = service_specific
                    .then_some("service-specific ")
                    .unwrap_or_default();
                write!(f, "{service_specific}profile `{name}`, key: `{key}`")
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
///
/// For a usage example, see [`crate::default_provider::retry_config`]
#[derive(Default)]
pub(crate) struct StandardProperty {
    environment_variable: Option<Cow<'static, str>>,
    profile_key: Option<Cow<'static, str>>,
    service_id: Option<Cow<'static, str>>,
}

impl StandardProperty {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Set the environment variable to read
    pub(crate) fn env(mut self, key: &'static str) -> Self {
        self.environment_variable = Some(Cow::Borrowed(key));
        self
    }

    /// Set the profile key to read
    pub(crate) fn profile(mut self, key: &'static str) -> Self {
        self.profile_key = Some(Cow::Borrowed(key));
        self
    }

    #[allow(dead_code)]
    /// Set the service id to check for service config
    pub(crate) fn service_id(mut self, service_id: &'static str) -> Self {
        self.service_id = Some(Cow::Borrowed(service_id));
        self
    }

    /// Load the value from `provider_config`, validating with `validator`
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

    /// Load the value from `provider_config`
    pub(crate) async fn load<'a>(
        &'a self,
        provider_config: &'a ProviderConfig,
    ) -> Option<(Cow<'a, str>, PropertySource<'a>)> {
        let env_value = self.environment_variable.as_ref().and_then(|env_var| {
            // Check for a service-specific env var first
            get_service_config_from_env(provider_config, self.service_id.as_ref(), env_var)
                // Then check for a global env var
                .or_else(|| {
                    provider_config.env().get(env_var).ok().map(|value| {
                        (
                            Cow::Owned(value),
                            PropertySource::Environment {
                                name: env_var,
                                service_specific: false,
                            },
                        )
                    })
                })
        });

        let profile = provider_config.profile().await?;
        let profile_value = self.profile_key.as_ref().and_then(|profile_key| {
            // Check for a service-specific profile key first
            get_service_config_from_profile(profile, self.service_id.as_ref(), profile_key)
                // Then check for a global profile key
                .or_else(|| {
                    profile.get(profile_key).map(|value| {
                        (
                            Cow::Borrowed(value),
                            PropertySource::Profile {
                                name: profile.selected_profile(),
                                key: profile_key,
                                service_specific: false,
                            },
                        )
                    })
                })
        });

        env_value.or(profile_value)
    }
}
fn get_service_config_from_env<'a>(
    provider_config: &'a ProviderConfig,
    service_id: Option<impl AsRef<str>>,
    env_var: &'a str,
) -> Option<(Cow<'a, str>, PropertySource<'a>)> {
    let service_specific_env_key = service_id
        .as_ref()
        .map(format_service_id_for_env)
        .map(|service_id| format!("{env_var}_{service_id}"))?;

    provider_config
        .env()
        .get(&service_specific_env_key)
        .ok()
        .map(|value| {
            (
                Cow::Owned(value),
                PropertySource::Environment {
                    name: env_var,
                    service_specific: true,
                },
            )
        })
}

fn get_service_config_from_profile<'a>(
    profile: &'a ProfileSet,
    service_id: Option<impl AsRef<str>>,
    profile_key: &'a str,
) -> Option<(Cow<'a, str>, PropertySource<'a>)> {
    let service_id = service_id.map(format_service_id_for_profile)?;
    let services_section_name = profile.get("services")?;
    let properties_key = PropertiesKey::builder()
        .section_key("services")
        .section_name(services_section_name)
        .property_name(service_id)
        .sub_property_name(profile_key)
        .build()
        .ok()?;
    let value = profile.other_sections().get(&properties_key)?;

    Some((
        Cow::Borrowed(value),
        PropertySource::Profile {
            name: profile.selected_profile(),
            key: profile_key,
            service_specific: true,
        },
    ))
}

fn format_service_id_for_env(service_id: impl AsRef<str>) -> String {
    service_id.as_ref().to_uppercase().replace(' ', "_")
}

fn format_service_id_for_profile(service_id: impl AsRef<str>) -> String {
    service_id.as_ref().to_lowercase().replace(' ', "-")
}

#[cfg(test)]
mod test {
    use super::StandardProperty;
    use crate::provider_config::ProviderConfig;
    use aws_types::os_shim_internal::{Env, Fs};
    use std::num::ParseIntError;

    fn validate_some_key(s: &str) -> Result<i32, ParseIntError> {
        s.parse()
    }

    #[tokio::test]
    async fn test_service_config_multiple_services() {
        let env = Env::from_slice(&[
            ("AWS_CONFIG_FILE", "config"),
            ("AWS_SOME_KEY", "1"),
            ("AWS_SOME_KEY_SERVICE", "2"),
            ("AWS_SOME_KEY_ANOTHER_SERVICE", "3"),
        ]);
        let fs = Fs::from_slice(&[(
            "config",
            r#"[default]
some_key = 4
services = dev

[services dev]
service =
  some_key = 5
another_service =
  some_key = 6
"#,
        )]);

        let provider_config = ProviderConfig::no_configuration().with_env(env).with_fs(fs);
        let global_from_env = StandardProperty::new()
            .env("AWS_SOME_KEY")
            .profile("some_key")
            .validate(&provider_config, validate_some_key)
            .await
            .expect("config resolution succeeds");
        assert_eq!(Some(1), global_from_env);

        let service_from_env = StandardProperty::new()
            .env("AWS_SOME_KEY")
            .profile("some_key")
            .service_id("service")
            .validate(&provider_config, validate_some_key)
            .await
            .expect("config resolution succeeds");
        assert_eq!(Some(2), service_from_env);

        let other_service_from_env = StandardProperty::new()
            .env("AWS_SOME_KEY")
            .profile("some_key")
            .service_id("another_service")
            .validate(&provider_config, validate_some_key)
            .await
            .expect("config resolution succeeds");
        assert_eq!(Some(3), other_service_from_env);

        let global_from_profile = StandardProperty::new()
            .profile("some_key")
            .validate(&provider_config, validate_some_key)
            .await
            .expect("config resolution succeeds");
        assert_eq!(Some(4), global_from_profile);

        let service_from_profile = StandardProperty::new()
            .profile("some_key")
            .service_id("service")
            .validate(&provider_config, validate_some_key)
            .await
            .expect("config resolution succeeds");
        assert_eq!(Some(5), service_from_profile);

        let service_from_profile = StandardProperty::new()
            .profile("some_key")
            .service_id("another_service")
            .validate(&provider_config, validate_some_key)
            .await
            .expect("config resolution succeeds");
        assert_eq!(Some(6), service_from_profile);
    }

    #[tokio::test]
    async fn test_service_config_precedence() {
        let env = Env::from_slice(&[
            ("AWS_CONFIG_FILE", "config"),
            ("AWS_SOME_KEY", "1"),
            ("AWS_SOME_KEY_S3", "2"),
        ]);
        let fs = Fs::from_slice(&[(
            "config",
            r#"[default]
some_key = 3
services = dev

[services dev]
s3 =
  some_key = 4
"#,
        )]);

        let provider_config = ProviderConfig::no_configuration().with_env(env).with_fs(fs);
        let global_from_env = StandardProperty::new()
            .env("AWS_SOME_KEY")
            .profile("some_key")
            .validate(&provider_config, validate_some_key)
            .await
            .expect("config resolution succeeds");
        assert_eq!(Some(1), global_from_env);

        let service_from_env = StandardProperty::new()
            .env("AWS_SOME_KEY")
            .profile("some_key")
            .service_id("s3")
            .validate(&provider_config, validate_some_key)
            .await
            .expect("config resolution succeeds");
        assert_eq!(Some(2), service_from_env);

        let global_from_profile = StandardProperty::new()
            .profile("some_key")
            .validate(&provider_config, validate_some_key)
            .await
            .expect("config resolution succeeds");
        assert_eq!(Some(3), global_from_profile);

        let service_from_profile = StandardProperty::new()
            .profile("some_key")
            .service_id("s3")
            .validate(&provider_config, validate_some_key)
            .await
            .expect("config resolution succeeds");
        assert_eq!(Some(4), service_from_profile);
    }
}
