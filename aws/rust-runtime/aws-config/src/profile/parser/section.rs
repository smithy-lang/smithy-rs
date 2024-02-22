/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::profile::parser::parse::to_ascii_lowercase;
use std::collections::HashMap;

/// Key-Value property pair
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Property {
    key: String,
    value: String,
}

impl Property {
    /// Value of this property
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Name of this property
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Creates a new property
    pub fn new(key: String, value: String) -> Self {
        Property { key, value }
    }
}

type SectionKey = String;
type SectionName = String;
type PropertyName = String;
type SubPropertyName = String;
type PropertyValue = String;

// [section-key section-name]
// property-name = property-value
// property-name =
//   sub-property-name = property-value
pub(crate) type PropertiesKey = (
    SectionKey,
    SectionName,
    PropertyName,
    Option<SubPropertyName>,
);
pub(crate) fn new_properties_key(
    section_key: &str,
    section_name: &str,
    property_name: &str,
    sub_property_name: Option<&str>,
) -> PropertiesKey {
    (
        section_key.to_owned(),
        section_name.to_owned(),
        property_name.to_owned(),
        sub_property_name.map(ToOwned::to_owned),
    )
}

fn format_properties_key(properties_key: &PropertiesKey) -> String {
    let (section_key, section_name, property_name, sub_property_name) = properties_key;
    match sub_property_name {
        Some(sub_property_name) => {
            format!("[{section_key} {section_name}].{property_name}.{sub_property_name}")
        }
        None => format!("[{section_key} {section_name}].{property_name}"),
    }
}

#[allow(clippy::type_complexity)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Properties {
    inner: HashMap<PropertiesKey, PropertyValue>,
}

#[allow(dead_code)]
impl Properties {
    pub(crate) fn new() -> Self {
        Default::default()
    }

    pub(crate) fn insert(&mut self, properties_key: &PropertiesKey, value: PropertyValue) {
        let _ = self
            .inner
            .entry(properties_key.clone())
            .and_modify(|v| {
                let formatted_key = format_properties_key(properties_key);
                tracing::trace!("overwriting {formatted_key}: was {v}, now {value}");

                *v = value.clone();
            })
            .or_insert(value);
    }

    pub(crate) fn get(&self, properties_key: &PropertiesKey) -> Option<&PropertyValue> {
        self.inner.get(properties_key)
    }
}

/// Represents a top-level section (e.g., `[profile name]`) in a config file.
pub(crate) trait Section {
    /// The name of this section
    fn name(&self) -> &str;

    /// Returns all the properties in this section
    fn properties(&self) -> &HashMap<String, Property>;

    /// Returns a reference to the property named `name`
    fn get(&self, name: &str) -> Option<&str>;

    /// True if there are no properties in this section.
    fn is_empty(&self) -> bool;

    /// Insert a property into a section
    fn insert(&mut self, name: String, value: Property);
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) struct SectionInner {
    pub(super) name: String,
    pub(super) properties: HashMap<String, Property>,
}

impl Section for SectionInner {
    fn name(&self) -> &str {
        &self.name
    }

    fn properties(&self) -> &HashMap<String, Property> {
        &self.properties
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.properties
            .get(to_ascii_lowercase(name).as_ref())
            .map(|prop| prop.value())
    }

    fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    fn insert(&mut self, name: String, value: Property) {
        self.properties
            .insert(to_ascii_lowercase(&name).into(), value);
    }
}

/// An individual configuration profile
///
/// An AWS config may be composed of a multiple named profiles within a [`ProfileSet`](crate::profile::ProfileSet).
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Profile(SectionInner);

impl Profile {
    /// Create a new profile
    pub fn new(name: impl Into<String>, properties: HashMap<String, Property>) -> Self {
        Self(SectionInner {
            name: name.into(),
            properties,
        })
    }

    /// The name of this profile
    pub fn name(&self) -> &str {
        self.0.name()
    }

    /// Returns a reference to the property named `name`
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name)
    }
}

impl Section for Profile {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn properties(&self) -> &HashMap<String, Property> {
        self.0.properties()
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name)
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn insert(&mut self, name: String, value: Property) {
        self.0.insert(name, value)
    }
}

/// A `[sso-session name]` section in the config.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct SsoSession(SectionInner);

impl SsoSession {
    /// Create a new SSO session section.
    pub(super) fn new(name: impl Into<String>, properties: HashMap<String, Property>) -> Self {
        Self(SectionInner {
            name: name.into(),
            properties,
        })
    }

    /// Returns a reference to the property named `name`
    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name)
    }
}

impl Section for SsoSession {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn properties(&self) -> &HashMap<String, Property> {
        self.0.properties()
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name)
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn insert(&mut self, name: String, value: Property) {
        self.0.insert(name, value)
    }
}

#[cfg(test)]
mod test {
    use crate::profile::parser::section::new_properties_key;
    use crate::provider_config::ProviderConfig;
    use aws_types::os_shim_internal::{Env, Fs};

    #[tokio::test]
    async fn test_other_properties_path_get() {
        let _ = tracing_subscriber::fmt::try_init();
        const CFG: &str = r#"[default]
services = foo

[services foo]
s3 =
  endpoint_url = http://localhost:3000
  setting_a = foo
  setting_b = bar

ec2 =
  endpoint_url = http://localhost:2000
  setting_a = foo

[services bar]
ec2 =
  endpoint_url = http://localhost:3000
  setting_b = bar
"#;
        let env = Env::from_slice(&[("AWS_CONFIG_FILE", "config")]);
        let fs = Fs::from_slice(&[("config", CFG)]);

        let provider_config = ProviderConfig::no_configuration().with_env(env).with_fs(fs);

        let p = provider_config.try_profile().await.unwrap();
        let other_sections = p.other_sections();

        assert_eq!(
            "http://localhost:3000",
            other_sections
                .get(&new_properties_key(
                    "services",
                    "foo",
                    "s3",
                    Some("endpoint_url")
                ))
                .expect("setting exists at path")
        );
        assert_eq!(
            "foo",
            other_sections
                .get(&new_properties_key(
                    "services",
                    "foo",
                    "s3",
                    Some("setting_a")
                ))
                .expect("setting exists at path")
        );
        assert_eq!(
            "bar",
            other_sections
                .get(&new_properties_key(
                    "services",
                    "foo",
                    "s3",
                    Some("setting_b")
                ))
                .expect("setting exists at path")
        );

        assert_eq!(
            "http://localhost:2000",
            other_sections
                .get(&new_properties_key(
                    "services",
                    "foo",
                    "ec2",
                    Some("endpoint_url")
                ))
                .expect("setting exists at path")
        );
        assert_eq!(
            "foo",
            other_sections
                .get(&new_properties_key(
                    "services",
                    "foo",
                    "ec2",
                    Some("setting_a")
                ))
                .expect("setting exists at path")
        );

        assert_eq!(
            "http://localhost:3000",
            other_sections
                .get(&new_properties_key(
                    "services",
                    "bar",
                    "ec2",
                    Some("endpoint_url")
                ))
                .expect("setting exists at path")
        );
        assert_eq!(
            "bar",
            other_sections
                .get(&new_properties_key(
                    "services",
                    "bar",
                    "ec2",
                    Some("setting_b")
                ))
                .expect("setting exists at path")
        );
    }
}
