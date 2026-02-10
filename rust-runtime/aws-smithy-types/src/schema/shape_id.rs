/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

/// A Smithy Shape ID.
///
/// Shape IDs uniquely identify shapes in a Smithy model.
/// Format: `namespace#shapeName` or `namespace#shapeName$memberName`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShapeId {
    value: String,
}

impl ShapeId {
    /// Creates a new ShapeId from a string.
    ///
    /// # Examples
    /// ```
    /// use aws_smithy_types::schema::ShapeId;
    ///
    /// let shape_id = ShapeId::new("smithy.api#String");
    /// ```
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }

    /// Returns the string representation of this ShapeId.
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Returns the namespace portion of the ShapeId.
    ///
    /// # Examples
    /// ```
    /// use aws_smithy_types::schema::ShapeId;
    ///
    /// let shape_id = ShapeId::new("smithy.api#String");
    /// assert_eq!(shape_id.namespace(), Some("smithy.api"));
    /// ```
    pub fn namespace(&self) -> Option<&str> {
        self.value.split_once('#').map(|(ns, _)| ns)
    }

    /// Returns the shape name portion of the ShapeId.
    ///
    /// # Examples
    /// ```
    /// use aws_smithy_types::schema::ShapeId;
    ///
    /// let shape_id = ShapeId::new("smithy.api#String");
    /// assert_eq!(shape_id.shape_name(), Some("String"));
    /// ```
    pub fn shape_name(&self) -> Option<&str> {
        self.value
            .split_once('#')
            .and_then(|(_, rest)| rest.split_once('$').map(|(name, _)| name).or(Some(rest)))
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
        self.value
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
        Self::new(value)
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
        assert_eq!(
            ShapeId::new("smithy.api#String").namespace(),
            Some("smithy.api")
        );
        assert_eq!(
            ShapeId::new("com.example#MyStruct$member").namespace(),
            Some("com.example")
        );
        assert_eq!(ShapeId::new("NoNamespace").namespace(), None);
    }

    #[test]
    fn test_shape_name() {
        assert_eq!(
            ShapeId::new("smithy.api#String").shape_name(),
            Some("String")
        );
        assert_eq!(
            ShapeId::new("com.example#MyStruct$member").shape_name(),
            Some("MyStruct")
        );
        assert_eq!(ShapeId::new("NoNamespace").shape_name(), None);
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
