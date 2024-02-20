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
type SubPropertyGroupName = String;
type SubPropertyKey = String;
type SubPropertyValue = String;

#[allow(clippy::type_complexity)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Properties {
    // [section-key section-name]
    // sub-property-group-name =
    //   sub-property-name = sub-property-value
    inner: HashMap<
        SectionKey,
        HashMap<
            SectionName,
            HashMap<SubPropertyGroupName, HashMap<SubPropertyKey, SubPropertyValue>>,
        >,
    >,
}

impl Properties {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn insert_at(&mut self, path: &[&str], value: String) {
        if path.is_empty() {
            return;
        }

        if path.len() != 4 {
            tracing::warn!("Properties::insert_at only supports 4-item paths");
            return;
        }

        if let [section_key, section_name, sub_property_group_name, sub_property_key] = path {
            let section_by_key = self.inner.entry(section_key.to_string()).or_default();
            let section_by_name = section_by_key.entry(section_name.to_string()).or_default();
            let sub_property_group_by_name = section_by_name
                .entry(sub_property_group_name.to_string())
                .or_default();
            let _ = sub_property_group_by_name.entry(sub_property_key.to_string()).and_modify(|v| {
            tracing::trace!("overwriting [{section_key} {section_name}].{sub_property_group_name}.{sub_property_key}: was {v}, now {value}");
            *v = value.clone();
        })
            .or_insert(value);
        } else {
            unreachable!("we asserted the expected length before the pattern match")
        }
    }

    pub fn get_at(&self, path: &[&str]) -> Option<&String> {
        match *path {
            [] => None,
            [section_key, section_name, sub_property_group_name, sub_property_key] => {
                let section_by_key = self.inner.get(section_key)?;
                let section_by_name = section_by_key.get(section_name)?;
                let sub_property_group_by_name = section_by_name.get(sub_property_group_name)?;
                sub_property_group_by_name.get(sub_property_key)
            }
            _ => {
                tracing::warn!("Properties::get_at only supports 4-item paths");
                None
            }
        }
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
/// An AWS config may be composed of a multiple named profiles within a [`ProfileSet`].
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
                .get_at(&["services", "foo", "s3", "endpoint_url"])
                .expect("setting exists at path")
        );
        assert_eq!(
            "foo",
            other_sections
                .get_at(&["services", "foo", "s3", "setting_a"])
                .expect("setting exists at path")
        );
        assert_eq!(
            "bar",
            other_sections
                .get_at(&["services", "foo", "s3", "setting_b"])
                .expect("setting exists at path")
        );

        assert_eq!(
            "http://localhost:2000",
            other_sections
                .get_at(&["services", "foo", "ec2", "endpoint_url"])
                .expect("setting exists at path")
        );
        assert_eq!(
            "foo",
            other_sections
                .get_at(&["services", "foo", "ec2", "setting_a"])
                .expect("setting exists at path")
        );

        assert_eq!(
            "http://localhost:3000",
            other_sections
                .get_at(&["services", "bar", "ec2", "endpoint_url"])
                .expect("setting exists at path")
        );
        assert_eq!(
            "bar",
            other_sections
                .get_at(&["services", "bar", "ec2", "setting_b"])
                .expect("setting exists at path")
        );
    }
}
