/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Sections within an AWS config profile.

use std::collections::HashMap;
use std::fmt;

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
    /// Create a new [`PropertiseKeyBuilder`].
    pub fn builder() -> PropertiesKeyBuilder {
        Default::default()
    }
}

impl fmt::Display for PropertiesKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let PropertiesKey {
            section_key,
            section_name,
            property_name,
            sub_property_name,
        } = self;
        match sub_property_name {
            Some(sub_property_name) => {
                write!(
                    f,
                    "[{section_key} {section_name}].{property_name}.{sub_property_name}"
                )
            }
            None => {
                write!(f, "[{section_key} {section_name}].{property_name}")
            }
        }
    }
}

/// Builder for [`PropertiesKey`]s.
#[derive(Debug, Default)]
pub struct PropertiesKeyBuilder {
    section_key: Option<SectionKey>,
    section_name: Option<SectionName>,
    property_name: Option<PropertyName>,
    sub_property_name: Option<SubPropertyName>,
}

impl PropertiesKeyBuilder {
    /// Set the section key for this builder.
    pub fn section_key(mut self, section_key: impl Into<String>) -> Self {
        self.section_key = Some(section_key.into());
        self
    }

    /// Set the section name for this builder.
    pub fn section_name(mut self, section_name: impl Into<String>) -> Self {
        self.section_name = Some(section_name.into());
        self
    }

    /// Set the property name for this builder.
    pub fn property_name(mut self, property_name: impl Into<String>) -> Self {
        self.property_name = Some(property_name.into());
        self
    }

    /// Set the sub-property name for this builder.
    pub fn sub_property_name(mut self, sub_property_name: impl Into<String>) -> Self {
        self.sub_property_name = Some(sub_property_name.into());
        self
    }

    /// Build this builder. If all required fields are set,
    /// `Ok(PropertiesKey)` is returned. Otherwise, an error is returned.
    pub fn build(self) -> Result<PropertiesKey, String> {
        Ok(PropertiesKey {
            section_key: self
                .section_key
                .ok_or("A section_key is required".to_owned())?,
            section_name: self
                .section_name
                .ok_or("A section_name is required".to_owned())?,
            property_name: self
                .property_name
                .ok_or("A property_name is required".to_owned())?,
            sub_property_name: self.sub_property_name,
        })
    }
}

/// A map of [`PropertiesKey`]s to property values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Properties {
    inner: HashMap<PropertiesKey, PropertyValue>,
}

#[allow(dead_code)]
impl Properties {
    /// Create a new empty [`Properties`].
    pub fn new() -> Self {
        Default::default()
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
            .get(name.to_ascii_lowercase().as_str())
            .map(|prop| prop.value())
    }

    fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    fn insert(&mut self, name: String, value: Property) {
        self.properties.insert(name.to_ascii_lowercase(), value);
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
pub struct SsoSession(SectionInner);

impl SsoSession {
    /// Create a new SSO session section.
    pub(super) fn new(name: impl Into<String>, properties: HashMap<String, Property>) -> Self {
        Self(SectionInner {
            name: name.into(),
            properties,
        })
    }

    /// Returns a reference to the property named `name`
    pub fn get(&self, name: &str) -> Option<&str> {
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

// TODO
// #[cfg(test)]
// mod test {
//     use super::PropertiesKey;
//     use aws_types::os_shim_internal::{Env, Fs};
//
//     #[tokio::test]
//     async fn test_other_properties_path_get() {
//         let _ = tracing_subscriber::fmt::try_init();
//         const CFG: &str = r#"[default]
// services = foo
//
// [services foo]
// s3 =
//   endpoint_url = http://localhost:3000
//   setting_a = foo
//   setting_b = bar
//
// ec2 =
//   endpoint_url = http://localhost:2000
//   setting_a = foo
//
// [services bar]
// ec2 =
//   endpoint_url = http://localhost:3000
//   setting_b = bar
// "#;
//         let env = Env::from_slice(&[("AWS_CONFIG_FILE", "config")]);
//         let fs = Fs::from_slice(&[("config", CFG)]);
//
//         let provider_config = ProviderConfig::no_configuration().with_env(env).with_fs(fs);
//
//         let p = provider_config.try_profile().await.unwrap();
//         let other_sections = p.other_sections();
//
//         assert_eq!(
//             "http://localhost:3000",
//             other_sections
//                 .get(&PropertiesKey {
//                     section_key: "services".to_owned(),
//                     section_name: "foo".to_owned(),
//                     property_name: "s3".to_owned(),
//                     sub_property_name: Some("endpoint_url".to_owned())
//                 })
//                 .expect("setting exists at path")
//         );
//         assert_eq!(
//             "foo",
//             other_sections
//                 .get(&PropertiesKey {
//                     section_key: "services".to_owned(),
//                     section_name: "foo".to_owned(),
//                     property_name: "s3".to_owned(),
//                     sub_property_name: Some("setting_a".to_owned())
//                 })
//                 .expect("setting exists at path")
//         );
//         assert_eq!(
//             "bar",
//             other_sections
//                 .get(&PropertiesKey {
//                     section_key: "services".to_owned(),
//                     section_name: "foo".to_owned(),
//                     property_name: "s3".to_owned(),
//                     sub_property_name: Some("setting_b".to_owned())
//                 })
//                 .expect("setting exists at path")
//         );
//
//         assert_eq!(
//             "http://localhost:2000",
//             other_sections
//                 .get(&PropertiesKey {
//                     section_key: "services".to_owned(),
//                     section_name: "foo".to_owned(),
//                     property_name: "ec2".to_owned(),
//                     sub_property_name: Some("endpoint_url".to_owned())
//                 })
//                 .expect("setting exists at path")
//         );
//         assert_eq!(
//             "foo",
//             other_sections
//                 .get(&PropertiesKey {
//                     section_key: "services".to_owned(),
//                     section_name: "foo".to_owned(),
//                     property_name: "ec2".to_owned(),
//                     sub_property_name: Some("setting_a".to_owned())
//                 })
//                 .expect("setting exists at path")
//         );
//
//         assert_eq!(
//             "http://localhost:3000",
//             other_sections
//                 .get(&PropertiesKey {
//                     section_key: "services".to_owned(),
//                     section_name: "bar".to_owned(),
//                     property_name: "ec2".to_owned(),
//                     sub_property_name: Some("endpoint_url".to_owned())
//                 })
//                 .expect("setting exists at path")
//         );
//         assert_eq!(
//             "bar",
//             other_sections
//                 .get(&PropertiesKey {
//                     section_key: "services".to_owned(),
//                     section_name: "bar".to_owned(),
//                     property_name: "ec2".to_owned(),
//                     sub_property_name: Some("setting_b".to_owned())
//                 })
//                 .expect("setting exists at path")
//         );
//     }
// }
