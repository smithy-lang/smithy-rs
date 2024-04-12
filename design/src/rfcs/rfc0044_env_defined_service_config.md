# RFC: Environment-defined service configuration

> Status: RFC
>
> Applies to: client

For a summarized list of proposed changes, see the [Changes
Checklist](#changes-checklist) section.

In the AWS SDK for Rust today, customers are limited to setting global
configuration variables in their environment; They cannot set service-specific
variables. Other SDKs and the AWS CLI do allow for setting service-specific
variables.

This RFC proposes an implementation that would enable users to set
service-specific variables in their environment.

## Terminology

* **Global configuration**: configuration which will be used for requests to any
  service. May be overridden by service-specific configuration.
* **Service-specific configuration**: configuration which will be used for
  requests only to a specific service.
* **Configuration variable**: A key-value pair that defines configuration e.g.
  `key = value`, `key: value`, `KEY=VALUE`, etc.
    * Key and value as used in this RFC refer to each half of a configuration
      variable.
* **Sub-properties**: When parsing config variables from a profile file,
  sub-properties are a newline-delimited list of key-value pairs in an indented
  block following a `<service name>=\n` line. _For an example, see the **Profile
  File Configuration** section of this RFC where sub-properties are declared for
  two different services._

## The user experience if this RFC is implemented

While users can already set global configuration in their environment, this RFC
proposes two new ways to set service-specific configuration in their
environment.

### Environment Variables

When defining service-specific configuration with an environment variable, all
keys are formatted like so:

```sh
"AWS" + "_" + "<config key in CONST_CASE>" + "_" + "<service ID in CONST_CASE>"
```

As an example, setting an endpoint URL for different services would look like
this:

```sh
export AWS_ENDPOINT_URL=http://localhost:4444
export AWS_ENDPOINT_URL_ELASTICBEANSTALK=http://localhost:5555
export AWS_ENDPOINT_URL_DYNAMODB=http://localhost:6666
```

The first variable sets a global endpoint URL. The second variable overrides the
first variable, but only for the Elastic Beanstalk service. The third variable
overrides the first variable, but only for the DynamoDB service.

### Profile File Configuration

When defining service-specific configuration in a profile file, it looks like
this:

```ignore
[profile dev]
services = testing-s3-and-eb
endpoint_url = http://localhost:9000

[services testing-s3-and-eb]
s3 =
  endpoint_url = http://localhost:4567
elasticbeanstalk =
  endpoint_url = http://localhost:8000
```

When `dev` is the active profile, all services will use the
`http://localhost:9000` endpoint URL except where it is overridden. Because the
`dev` profile references the `testing-s3-and-eb` services, and because two
service-specific endpoint URLs are set, those URLs will override the
`http://localhost:9000` endpoint URL when making requests to S3
(`http://localhost:4567`) and Elastic Beanstalk (`http://localhost:8000`).

### Configuration Precedence

When configuration is set in multiple places, the value used is determined in
this order of precedence:

*highest precedence*

1. *EXISTING* Programmatic client configuration
2. *NEW* Service-specific environment variables
3. *EXISTING* Global environment variables
4. *NEW* Service-specific profile file variables in the active profile
5. *EXISTING* Global profile file variables in the active profile

*lowest precedence*

How to actually implement this RFC
----------------------------------

This RFC may be implemented in several steps which are detailed below.

### Sourcing service-specific config from the environment and profile

`aws_config::profile::parser::ProfileSet` is responsible for storing the active
profile and all profile configuration data. Currently, it only tracks
`sso_session` and `profile` sections, so it must be updated to store arbitrary
sections, their properties, and sub-properties. These sections will be publicly
accessible via a new method `ProfileSet::other_sections` which returns a ref to
a `Properties` struct.

The `Properties` struct is defined as follows:

```rust,ignore
type SectionKey = String;
type SectionName = String;
type PropertyName = String;
type SubPropertyName = String;
type PropertyValue = String;

/// A key for to a property value.
///
/// ```txt
/// # An example AWS profile config section with properties and sub-properties
/// [section-key section-name]
/// property-name = property-value
/// property-name =
///   sub-property-name = property-value
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PropertiesKey {
    section_key: SectionKey,
    section_name: SectionName,
    property_name: PropertyName,
    sub_property_name: Option<SubPropertyName>,
}

impl PropertiesKey {
    /// Create a new builder for a `PropertiesKey`.
    pub fn builder() -> Builder {
        Default::default()
    }
}

// The builder code is omitted from this RFC. It allows users to set each field
// individually and then build a PropertiesKey

/// A map of [`PropertiesKey`]s to property values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Properties {
    inner: HashMap<PropertiesKey, PropertyValue>,
}

impl Properties {
    /// Create a new empty [`Properties`].
    pub fn new() -> Self {
        Default::default()
    }

    #[cfg(test)]
    pub(crate) fn new_from_slice(slice: &[(PropertiesKey, PropertyValue)]) -> Self {
        let mut properties = Self::new();
        for (key, value) in slice {
            properties.insert(key.clone(), value.clone());
        }
        properties
    }

    /// Insert a new key/value pair into this map.
    pub fn insert(&mut self, properties_key: PropertiesKey, value: PropertyValue) {
        let _ = self
            .inner
            // If we don't clone then we don't get to log a useful warning for a value getting overwritten.
            .entry(properties_key.clone())
            .and_modify(|v| {
                tracing::trace!("overwriting {properties_key}: was {v}, now {value}");
                *v = value.clone();
            })
            .or_insert(value);
    }

    /// Given a [`PropertiesKey`], return the corresponding value, if any.
    pub fn get(&self, properties_key: &PropertiesKey) -> Option<&PropertyValue> {
        self.inner.get(properties_key)
    }
}
```

The `aws_config::env` module remains unchanged. It already provides all the
necessary functionality.

### Exposing valid service configuration during `<service>::Config` construction

Environment variables *(from `Env`)* and profile variables *(from
`EnvConfigSections`)* must be available during the conversion of `SdkConfig` to
`<service>::Config`. To accomplish this, we'll define a new trait
`LoadServiceConfig` and implement it for `EnvServiceConfig`  which will be
stored in the `SdkConfig` struct.

```rust,ignore
/// A struct used with the [`LoadServiceConfig`] trait to extract service config from the user's environment.
// [profile active-profile]
// services = dev
//
// [services dev]
// service-id =
//   config-key = config-value
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServiceConfigKey<'a> {
    service_id: &'a str,
    profile: &'a str,
    env: &'a str,
}

impl<'a> ServiceConfigKey<'a> {
    /// Create a new [`ServiceConfigKey`] builder struct.
    pub fn builder() -> builder::Builder<'a> {
        Default::default()
    }
    /// Get the service ID.
    pub fn service_id(&self) -> &'a str {
        self.service_id
    }
    /// Get the profile key.
    pub fn profile(&self) -> &'a str {
        self.profile
    }
    /// Get the environment key.
    pub fn env(&self) -> &'a str {
        self.env
    }
}

/// Implementers of this trait can provide service config defined in a user's environment.
pub trait LoadServiceConfig: fmt::Debug + Send + Sync {
    /// Given a [`ServiceConfigKey`], return the value associated with it.
    fn load_config(&self, key: ServiceConfigKey<'_>) -> Option<String>;
}

#[derive(Debug)]
pub(crate) struct EnvServiceConfig {
    pub(crate) env: Env,
    pub(crate) env_config_sections: EnvConfigSections,
}

impl LoadServiceConfig for EnvServiceConfig {
    fn load_config(&self, key: ServiceConfigKey<'_>) -> Option<String> {
        let (value, _source) = EnvConfigValue::new()
            .env(key.env())
            .profile(key.profile())
            .service_id(key.service_id())
            .load(&self.env, Some(&self.env_config_sections))?;

        Some(value.to_string())
    }
}
```

### Code generation

We require two things to check for when constructing the service config:

- The service's ID
- The service's supported configuration variables

We **only** have this information once we get to the service level. Because of
that, we must use code generation to define:

- What config to look for in the environment
- How to validate that config

Codegen for configuration must be updated for all config variables that we want
to support. For an example, here's how we'd update the `RegionDecorator` to check
for service-specific regions:

```java
class RegionDecorator : ClientCodegenDecorator {
    // ...
    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        return usesRegion(codegenContext).thenSingletonListOf {
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rust(
                    """
                    ${section.serviceConfigBuilder}.set_region(
                        ${section.sdkConfig}
                            .service_config()
                            .and_then(|conf| {
                                conf.load_config(service_config_key($envKey, $profileKey))
                                    .map(Region::new)
                            })
                            .or_else(|| ${section.sdkConfig}.region().cloned()),
                    );
                    """,
                )
            }
        }
    }
    // ...
```

To construct the keys necessary to locate the service-specific configuration, we
generate a `service_config_key` function for each service crate:

```java
class ServiceEnvConfigDecorator : ClientCodegenDecorator {
    override val name: String = "ServiceEnvConfigDecorator"
    override val order: Byte = 10

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        val rc = codegenContext.runtimeConfig
        val serviceId = codegenContext.serviceShape.sdkId().toSnakeCase().dq()
        rustCrate.withModule(ClientRustModule.config) {
            Attribute.AllowDeadCode.render(this)
            rustTemplate(
                """
                fn service_config_key<'a>(
                    env: &'a str,
                    profile: &'a str,
                ) -> aws_types::service_config::ServiceConfigKey<'a> {
                    #{ServiceConfigKey}::builder()
                        .service_id($serviceId)
                        .env(env)
                        .profile(profile)
                        .build()
                        .expect("all field sets explicitly, can't fail")
                }
                """,
                "ServiceConfigKey" to AwsRuntimeType.awsTypes(rc).resolve("service_config::ServiceConfigKey"),
            )
        }
    }
}
```

## Changes checklist

- In `aws-types`:
  - [x] Add new `service_config: Option<Arc<dyn LoadServiceConfig>>` field to `SdkConfig` and builder.
  - [x] Add setters and getters for the new `service_config` field.
  - [x] Add a new `service_config` module.
    - [x] Add new `ServiceConfigKey` struct and builder.
    - [x] Add new `LoadServiceConfig` trait.
- In `aws-config`:
  - [x] Move profile parsing out of `aws-config` into `aws-runtime`.
  - [x] Deprecate the `aws-config` reëxports and direct users to `aws-runtime`.
  - [x] Add a new `EnvServiceConfig` struct and implement `LoadServiceConfig` for it.
  - [x] Update `ConfigLoader` to set the `service_config` field in `SdkConfig`.
  - [x] Update all default providers to use the new of the `EnvConfigValue::validate` method.
- In `aws-runtime`:
  - [x] Rename all profile-related code moved from `aws-config` to `aws-runtime` so that it's easier to understand in light of the API changes we're making.
  - [x] Add a new struct `PropertiesKey` and `Properties` to store profile data.
- [x] Add an integration test that ensures service-specific config has the expected precedence.
- [x] Update codegen to generate a method to easily construct `ServiceConfigKey`s.
- [x] Update codegen to generate code that loads service-specific config from the environment for a limited initial set of config variables:
  - Region
  - Endpoint URL
  - Endpoint-related "built-ins" like `use_arn_region` and `disable_multi_region_access_points`.
- [x] Write a [guide](https://github.com/smithy-lang/smithy-rs/discussions/3537) for users.
  - [x] Explain to users how they can determine a service's ID.
