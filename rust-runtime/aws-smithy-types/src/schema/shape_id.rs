/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::borrow::Cow;

/// A Smithy Shape ID.
///
/// Shape IDs uniquely identify shapes in a Smithy model.
/// - `fqn` is `"smithy.example#Foo"`
/// - `namespace` is `"smithy.example"`
/// - `shape_name` is `"Foo"`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapeId {
    fqn: Cow<'static, str>,
    namespace: Cow<'static, str>,
    shape_name: Cow<'static, str>,
}

impl ShapeId {
    /// Creates a ShapeId from a static string at compile time.
    ///
    /// This is used for const initialization of prelude schemas.
    pub const fn from_static(
        fqn: &'static str,
        namespace: &'static str,
        shape_name: &'static str,
    ) -> Self {
        Self {
            fqn: Cow::Borrowed(fqn),
            namespace: Cow::Borrowed(namespace),
            shape_name: Cow::Borrowed(shape_name),
        }
    }

    /// Creates a new ShapeId from a namespace and a shape_name.
    ///
    /// # Examples
    /// ```
    /// use aws_smithy_types::schema::ShapeId;
    ///
    /// let shape_id = ShapeId::new("smithy.api#String");
    /// ```
    pub fn new_from_parts(
        namespace: impl Into<Cow<'static, str>>,
        shape_name: impl Into<Cow<'static, str>>,
    ) -> Self {
        let namespace = namespace.into();
        let shape_name = shape_name.into();
        Self {
            fqn: format!("{}#{}", namespace.as_ref(), shape_name.as_ref()).into(),
            namespace,
            shape_name,
        }
    }

    /// Creates a new ShapeId from a fully qualified name.
    pub fn new_from_fqn(fqn: impl Into<Cow<'static, str>>) -> Option<Self> {
        let fqn = fqn.into();
        let (namespace, shape_name) = fqn.as_ref().split_once('#').map(|(ns, rest)| {
            (
                ns.to_string(),
                rest.split_once('$')
                    .map_or(rest, |(name, _)| name)
                    .to_string(),
            )
        })?;
        Some(Self {
            fqn,
            namespace: namespace.into(),
            shape_name: shape_name.into(),
        })
    }

    /// Creates a new ShapeId from a fully qualified name.
    pub fn new(fqn: impl Into<Cow<'static, str>>) -> Self {
        Self::new_from_fqn(fqn).expect("invalid shape ID")
    }

    /// Returns the string representation of this ShapeId.
    pub fn as_str(&self) -> &str {
        self.fqn.as_ref()
    }

    /// Returns the namespace portion of the ShapeId.
    ///
    /// # Examples
    /// ```
    /// use aws_smithy_types::schema::ShapeId;
    ///
    /// let shape_id = ShapeId::from_static("smithy.api#String");
    /// assert_eq!(shape_id.namespace(), Some("smithy.api"));
    /// ```
    pub fn namespace(&self) -> &str {
        self.namespace.as_ref()
    }

    /// Returns the shape name portion of the ShapeId.
    ///
    /// # Examples
    /// ```
    /// use aws_smithy_types::schema::ShapeId;
    ///
    /// let shape_id = ShapeId::from_static("smithy.api#String");
    /// assert_eq!(shape_id.shape_name(), Some("String"));
    /// ```
    pub fn shape_name(&self) -> &str {
        self.shape_name.as_ref()
    }

    /// Returns the member name if this is a member shape ID.
    ///
    /// # Examples
    /// ```
    /// use aws_smithy_types::schema::ShapeId;
    ///
    /// let shape_id = ShapeId::new("com.example#MyStruct$member");
    /// assert_eq!(shape_id.member_name(), Some("member"));
    /// ```
    pub fn member_name(&self) -> Option<&str> {
        self.fqn
            .split_once('#')
            .and_then(|(_, rest)| rest.split_once('$').map(|(_, member)| member))
    }
}

impl From<String> for ShapeId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for ShapeId {
    fn from(value: &str) -> Self {
        Self::new(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let shape_id = ShapeId::new("smithy.api#String");
        assert_eq!(shape_id.as_str(), "smithy.api#String");
    }

    #[test]
    fn test_namespace() {
        assert_eq!(ShapeId::new("smithy.api#String").namespace(), "smithy.api");
        assert_eq!(
            ShapeId::new("com.example#MyStruct$member").namespace(),
            "com.example"
        );
    }

    #[test]
    fn test_shape_name() {
        assert_eq!(ShapeId::new("smithy.api#String").shape_name(), "String");
        assert_eq!(
            ShapeId::new("com.example#MyStruct$member").shape_name(),
            "MyStruct"
        );
    }

    #[test]
    fn test_member_name() {
        assert_eq!(
            ShapeId::new("com.example#MyStruct$member").member_name(),
            Some("member")
        );
        assert_eq!(ShapeId::new("smithy.api#String").member_name(), None);
        assert_eq!(ShapeId::new("NoNamespace").member_name(), None);
    }

    #[test]
    fn test_from_string() {
        let shape_id: ShapeId = String::from("smithy.api#String").into();
        assert_eq!(shape_id.as_str(), "smithy.api#String");
    }

    #[test]
    fn test_from_str() {
        let shape_id: ShapeId = "smithy.api#String".into();
        assert_eq!(shape_id.as_str(), "smithy.api#String");
    }

    #[test]
    fn test_clone_and_equality() {
        let shape_id1 = ShapeId::new("smithy.api#String");
        let shape_id2 = shape_id1.clone();
        assert_eq!(shape_id1, shape_id2);
    }
}
