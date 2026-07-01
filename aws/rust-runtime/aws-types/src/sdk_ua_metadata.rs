/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Customer-facing types for self-identifying additional framework metadata in the user agent.

use aws_smithy_types::config_bag::{Storable, StoreAppend};
use std::borrow::Cow;
use std::error::Error;
use std::fmt;

/// Metadata about a software framework or third-party library that is being used with the SDK.
///
/// This is rendered into the user agent string (as `lib/{name}/{version}`) so that third-party
/// libraries built on top of the AWS SDK can self-identify in requests they make.
///
/// The name and version may only have alphanumeric characters and any of these characters:
/// ```text
/// !#$%&'*+-.^_`|~
/// ```
/// Spaces are not allowed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameworkMetadata {
    name: Cow<'static, str>,
    version: Option<Cow<'static, str>>,
}

impl Storable for FrameworkMetadata {
    type Storer = StoreAppend<Self>;
}

impl FrameworkMetadata {
    /// Creates new `FrameworkMetadata`.
    ///
    /// This will return an [`InvalidFrameworkMetadata`] error if the given name or version doesn't
    /// meet the character requirements. See [`FrameworkMetadata`] for details on these requirements.
    pub fn new(
        name: impl Into<Cow<'static, str>>,
        version: Option<impl Into<Cow<'static, str>>>,
    ) -> Result<Self, InvalidFrameworkMetadata> {
        let name = name.into();
        let version = version.map(Into::into);

        if name.is_empty() {
            return Err(InvalidFrameworkMetadata);
        }
        // Reject (do not sanitize) any character outside the permitted charset. This mirrors
        // `AppName::new` and prevents header injection via untrusted framework metadata.
        fn valid_character(c: char) -> bool {
            match c {
                _ if c.is_ascii_alphanumeric() => true,
                '!' | '#' | '$' | '%' | '&' | '\'' | '*' | '+' | '-' | '.' | '^' | '_' | '`'
                | '|' | '~' => true,
                _ => false,
            }
        }
        if !name.chars().all(valid_character) {
            return Err(InvalidFrameworkMetadata);
        }
        if let Some(version) = &version {
            if version.is_empty() {
                return Err(InvalidFrameworkMetadata);
            }
            if !version.chars().all(valid_character) {
                return Err(InvalidFrameworkMetadata);
            }
        }
        Ok(Self { name, version })
    }

    /// Returns the framework name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the framework version, if set.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
}

impl fmt::Display for FrameworkMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // framework-metadata = "lib/" name ["/" version]
        match &self.version {
            Some(version) => write!(f, "lib/{}/{}", self.name, version),
            None => write!(f, "lib/{}", self.name),
        }
    }
}

/// Error for when framework metadata doesn't meet character requirements.
///
/// See [`FrameworkMetadata`] for details on these requirements.
#[derive(Debug)]
#[non_exhaustive]
pub struct InvalidFrameworkMetadata;

impl Error for InvalidFrameworkMetadata {}

impl fmt::Display for InvalidFrameworkMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "The framework metadata name and version can only have alphanumeric characters, or \
             any of '!' |  '#' |  '$' |  '%' |  '&' |  '\\'' |  '*' |  '+' |  '-' | \
             '.' |  '^' |  '_' |  '`' |  '|' |  '~'"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::FrameworkMetadata;

    #[test]
    fn valid_name_and_version() {
        let md = FrameworkMetadata::new("some-framework", Some("1.0")).expect("valid");
        assert_eq!("some-framework", md.name());
        assert_eq!(Some("1.0"), md.version());
    }

    #[test]
    fn valid_name_no_version() {
        let md =
            FrameworkMetadata::new("asdf1234ASDF!#$%&'*+-.^_`|~", None::<&str>).expect("valid");
        assert_eq!(None, md.version());
    }

    #[test]
    fn invalid_charset_name() {
        assert!(FrameworkMetadata::new("foo bar", None::<&str>).is_err());
        assert!(FrameworkMetadata::new("🚀", None::<&str>).is_err());
    }

    #[test]
    fn invalid_charset_version() {
        assert!(FrameworkMetadata::new("framework", Some("1 0")).is_err());
        assert!(FrameworkMetadata::new("framework", Some("🚀")).is_err());
    }

    #[test]
    fn empty_version_rejected() {
        assert!(FrameworkMetadata::new("framework", Some("")).is_err());
    }

    #[test]
    fn empty_name() {
        assert!(FrameworkMetadata::new("", None::<&str>).is_err());
    }

    #[test]
    fn rejects_header_injection_characters() {
        // None of these may ever reach the user-agent header.
        for bad in [
            "a\r\nb",   // CRLF (header injection)
            "a\nb",     // LF
            "a\rb",     // CR
            "a\tb",     // tab
            "a b",      // space (separates UA tokens)
            "a/b",      // slash (would forge an extra lib/.../... segment)
            "a\u{0}b",  // NUL
            "a\u{7f}b", // DEL
        ] {
            assert!(
                FrameworkMetadata::new(bad, None::<&str>).is_err(),
                "name {bad:?} should be rejected"
            );
            assert!(
                FrameworkMetadata::new("framework", Some(bad)).is_err(),
                "version {bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn accepts_every_allowed_symbol_in_version() {
        let md =
            FrameworkMetadata::new("framework", Some("1.0-rc.1+build_2~3")).expect("valid version");
        assert_eq!(Some("1.0-rc.1+build_2~3"), md.version());
    }

    #[test]
    fn clone_and_equality() {
        let a = FrameworkMetadata::new("framework", Some("1.0")).unwrap();
        let b = a.clone();
        assert_eq!(a, b);
        let c = FrameworkMetadata::new("framework", Some("2.0")).unwrap();
        assert_ne!(a, c);
        let d = FrameworkMetadata::new("framework", None::<&str>).unwrap();
        assert_ne!(a, d);
    }
}
