RFC: Environment-defined service configuration
==============================================

> Status: RFC
>
> Applies to: client

For a summarized list of proposed changes, see the [Changes Checklist](#changes-checklist) section.

In the AWS SDK for Rust today, customers are limited to setting global configuration variables in their environment; They cannot set service-specific variables. Other SDKs and the AWS CLI do allow for setting service-specific variables.

This RFC proposes an implementation that would enable users to set service-specific variables in their environment.

Terminology
-----------

* **Global configuration**: configuration which will be used for requests to any service. May be overridden by service-specific configuration.
* **Service-specific configuration**: configuration which will be used for requests only to a specific service.
* **Configuration variable**: A key-value pair that defines configuration e.g. `key = value`, `key: value`, `KEY=VALUE`, etc.
    * Key and value as used in this RFC refer to each half of a configuration variable.
* **Sub-properties**: When parsing config variables from a profile file, sub-properties are a newline-delimited list of key-value pairs in an indented block following a `<service name>=\n` line. _For an example, see the **Profile File Configuration** section of this RFC where sub-properties are declared for two different services._

The user experience if this RFC is implemented
----------------------------------------------

While users can already set global configuration in their environment, this RFC proposes two new ways to set service-specific configuration in their environment.

### Environment Variables

When defining service-specific configuration with an environment variable, all keys are formatted like so:

```sh
"AWS" + "_" + "<config key in CONST_CASE>" + "_" + "<service ID in CONST_CASE>"
```

As an example, setting an endpoint URL for different services would look like this:

```sh
export AWS_ENDPOINT_URL=http://localhost:4567
export AWS_ENDPOINT_URL_ELASTIC_BEANSTALK=http://localhost:5567
export AWS_ENDPOINT_URL_DYNAMODB=http://localhost:5678
```

The first variable sets a global endpoint URL. The second variable overrides the first variable, but only for the Elastic Beanstalk service. The third variable overrides the first variable, but only for the DynamoDB service.

### Profile File Configuration

When defining service-specific configuration in a profile file, it looks like this:

```
[profile dev]
services = testing-s3-and-eb
endpoint_url = http://localhost:9000

[services testing-s3-and-eb]
s3 =
  endpoint_url = http://localhost:4567
elastic_beanstalk =
  endpoint_url = http://localhost:8000
```

When `dev` is the active profile, all services will use the `http://localhost:9000` endpoint URL except where it is overridden. Because the `dev` profile references the `testing-s3-and-eb` services, and because two service-specific endpoint URLs are set, those URLs will override the `http://localhost:9000` endpoint URL when making requests to S3 (`http://localhost:4567`) and Elastic Beanstalk (`http://localhost:8000`).

### Configuration Precedence

When configuration is set in multiple places, the value used is determined in this order of precedence:

*highest precedence*

1. Programmatic client configuration
2. Service-specific environment variables
3. Global environment variables
4. Service-specific profile file variables in the active profile
5. Global profile file variables in the active profile

*lowest precedence*

How to actually implement this RFC
----------------------------------

This RFC may be implemented in several steps which are detailed below.

### Sourcing service-specific config from the environment and profile

`aws_config::profile::parser::ProfileSet` is responsible for storing the active profile and all profile configuration data.
Currently, it only tracks `sso_session` and `profile` sections, so it must be updated to store arbitrary sections, their properties, and sub-properties.
These sections will be publicly accessible via a new method `ProfileSet::other_sections` which returns a ref to a `Properties` struct.

The `Properties` struct is defined as follows:

```rust
type SectionKey = String;
type SectionName = String;
type PropertyName = String;
type SubPropertyName = String;
type PropertyValue = String;

// [section-key section-name]
// property-name = property-value
// property-name =
//   sub-property-name = property-value
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
        Builder::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Properties {
    inner: HashMap<PropertiesKey, PropertyValue>,
}

impl Properties {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn insert(&mut self, properties_key: PropertiesKey, value: PropertyValue) {
        self
            .inner
            .entry(properties_key)
            .and_modify(|v| {
                tracing::trace!("overwriting {properties_key}: was {v}, now {value}");

                *v = value.clone();
            })
            .or_insert(value);
    }

    pub fn get(&self, properties_key: &PropertiesKey) -> Option<&PropertyValue> {
        self.inner.get(properties_key)
    }
}
```

The `aws_config::env` module remains unchanged. It already provides all the necessary functionality.

### Exposing valid service config during `<service>::Config` construction

The `ProviderConfig` must be available during `<service>::Config` construction. To accomplish this,
we'll define a new struct `ServiceEnvConfig` that contains a clone of `ProviderConfig` and store it in the
`SdkConfig` struct. Then, when the `SdkConfig` is converted into a `<service>::Config`, it can be queried
for known service configuration.

```rust
// [profile active-profile]
// services = dev
//
// [services dev]
// service-id =
//   config-key = config-value
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServiceEnvConfigKey<'a> {
    service_id: &'a str,
    profile: &'a str,
    env: &'a str,
}

impl ServiceEnvConfigKey<'a> {
    pub fn builder() -> Builder { Builder::default() }
    pub fn service_id(&self) -> &'a str { self.service_id }
    pub fn profile(&self) -> &'a str { self.profile }
    pub fn env(&self) -> &'a str { self.env }
}

#[derive(Clone, Debug)]
struct ServiceEnvConfig {
    provider_config: ProviderConfig,
}

impl ServiceEnvConfig {
    pub fn new(provider_config: ProviderConfig) -> Self {
        Self { provider_config }
    }

    pub async fn get<T, E>(
        key: ServiceEnvConfigKey<a'>,
        validator: impl Fn(&str) -> Result<T, E>,
    ) -> Result<T, PropertyResolutionError<E>>
    where
        E: Error + Send + Sync + 'static
    {
        StandardProperty::new()
            .env(key.env())
            .profile(key.profile())
            .service(key.service_id())
            .validate(&self.provider_config, validator)
            .await
            .map_err(
                |err| tracing::warn!(err = %DisplayErrorContext(&err), "invalid value for {} setting", key.profile()),
            )
    }
}
```

### Code generation

We require two things to check for when constructing the service config:

- The service's ID
- The service's supported configuration variables

We **only** have this information once we get to the service level. Because of that, we must use code generation to define:

- What config to look for in the environment
- How to validate that config

A new section will be added to `AdHocCustomization`'s `SdkConfigSection` called `ServiceEnvConfigLayer`. This new section will contain code that
inspects the `ServiceEnvConfig` returned by `SdkConfig::service_env_config` for config variables and assigns validators for each one.

The config variables extraced at this stage are then used to construct a config [`Layer`](https://docs.rs/aws-smithy-types/latest/aws_smithy_types/config_bag/struct.Layer.html).
A new `ServiceEnvConfigRuntimePlugin` will provide this layer when `<service>::Config` is being constructed. `Layer`s already support merging configuration.

### Disabling all config `endpoint_url`s

We must also support disabling all configured endpoint URLs with an environment variable or a profile setting.

* `AWS_IGNORE_CONFIGURED_ENDPOINT_URLS` is the environment variable.
* `ignore_configured_endpoint_urls` is the profile variable.

To support this, we'll add a new provider that mimics `use_fips_provider` and `use_dualstack_provider`.
`ConfigLoader` and `SdkConfig` will then be updated to support this provider.

Changes checklist
-----------------

TODO
