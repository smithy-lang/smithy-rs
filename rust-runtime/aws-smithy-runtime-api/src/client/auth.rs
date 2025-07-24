/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! APIs for request authentication.

use crate::box_error::BoxError;
use crate::client::identity::{Identity, SharedIdentityResolver};
use crate::client::orchestrator::HttpRequest;
use crate::client::runtime_components::sealed::ValidateConfig;
use crate::client::runtime_components::{GetIdentityResolver, RuntimeComponents};
use crate::impl_shared_conversions;
use aws_smithy_types::config_bag::{ConfigBag, FrozenLayer, Storable, StoreReplace};
use aws_smithy_types::type_erasure::TypeErasedBox;
use aws_smithy_types::Document;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

/// Auth schemes for the HTTP `Authorization` header.
#[cfg(feature = "http-auth")]
pub mod http;

/// Static auth scheme option resolver.
pub mod static_resolver;

/// The output type from the [`ResolveAuthSchemeOptions::resolve_auth_scheme_options_v2`]
///
/// The resolver returns a list of these, in the order the auth scheme resolver wishes to use them.
#[derive(Clone, Debug)]
pub struct AuthSchemeOption {
    scheme_id: AuthSchemeId,
    properties: Option<FrozenLayer>,
}

impl AuthSchemeOption {
    /// Builder struct for [`AuthSchemeOption`]
    pub fn builder() -> AuthSchemeOptionBuilder {
        AuthSchemeOptionBuilder::default()
    }

    /// Returns [`AuthSchemeId`], the ID of the scheme
    pub fn scheme_id(&self) -> &AuthSchemeId {
        &self.scheme_id
    }

    /// Returns optional properties for identity resolution or signing
    ///
    /// This config layer is applied to the [`ConfigBag`] to ensure the information is
    /// available during both the identity resolution and signature generation processes.
    pub fn properties(&self) -> Option<FrozenLayer> {
        self.properties.clone()
    }
}

impl From<AuthSchemeId> for AuthSchemeOption {
    fn from(auth_scheme_id: AuthSchemeId) -> Self {
        AuthSchemeOption::builder()
            .scheme_id(auth_scheme_id)
            .build()
            .expect("required fields set")
    }
}

/// Builder struct for [`AuthSchemeOption`]
#[derive(Debug, Default)]
pub struct AuthSchemeOptionBuilder {
    scheme_id: Option<AuthSchemeId>,
    properties: Option<FrozenLayer>,
}

impl AuthSchemeOptionBuilder {
    /// Sets [`AuthSchemeId`] for the builder
    pub fn scheme_id(mut self, auth_scheme_id: AuthSchemeId) -> Self {
        self.set_scheme_id(Some(auth_scheme_id));
        self
    }

    /// Sets [`AuthSchemeId`] for the builder
    pub fn set_scheme_id(&mut self, auth_scheme_id: Option<AuthSchemeId>) {
        self.scheme_id = auth_scheme_id;
    }

    /// Sets the properties for the builder
    pub fn properties(mut self, properties: FrozenLayer) -> Self {
        self.set_properties(Some(properties));
        self
    }

    /// Sets the properties for the builder
    pub fn set_properties(&mut self, properties: Option<FrozenLayer>) {
        self.properties = properties;
    }

    /// Builds an [`AuthSchemeOption`], otherwise returns an [`AuthSchemeOptionBuilderError`] in the case of error
    pub fn build(self) -> Result<AuthSchemeOption, AuthSchemeOptionBuilderError> {
        let scheme_id = self
            .scheme_id
            .ok_or(ErrorKind::MissingRequiredField("auth_scheme_id"))?;
        Ok(AuthSchemeOption {
            scheme_id,
            properties: self.properties,
        })
    }
}

#[derive(Debug)]
enum ErrorKind {
    MissingRequiredField(&'static str),
}

impl From<ErrorKind> for AuthSchemeOptionBuilderError {
    fn from(kind: ErrorKind) -> Self {
        Self { kind }
    }
}

/// The error type returned when failing to build [`AuthSchemeOption`] from the builder
#[derive(Debug)]
pub struct AuthSchemeOptionBuilderError {
    kind: ErrorKind,
}

impl fmt::Display for AuthSchemeOptionBuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ErrorKind::MissingRequiredField(name) => {
                write!(f, "`{name}` is required")
            }
        }
    }
}

impl std::error::Error for AuthSchemeOptionBuilderError {}

/// New type around an auth scheme ID.
///
/// Each auth scheme must have a unique string identifier associated with it,
/// which is used to refer to auth schemes by the auth scheme option resolver, and
/// also used to select an identity resolver to use.
#[derive(Clone, Debug, Eq)]
pub struct AuthSchemeId {
    scheme_id: Cow<'static, str>,
}

// See: https://doc.rust-lang.org/std/convert/trait.AsRef.html#reflexivity
impl AsRef<AuthSchemeId> for AuthSchemeId {
    fn as_ref(&self) -> &AuthSchemeId {
        self
    }
}

impl AuthSchemeId {
    /// Creates a new auth scheme ID.
    pub const fn new(scheme_id: &'static str) -> Self {
        Self {
            scheme_id: Cow::Borrowed(scheme_id),
        }
    }

    /// Returns the string equivalent of this auth scheme ID.
    #[deprecated(
        note = "This function is no longer functional. Use `inner` instead",
        since = "1.8.0"
    )]
    pub const fn as_str(&self) -> &'static str {
        match self.scheme_id {
            Cow::Borrowed(val) => val,
            Cow::Owned(_) => {
                // cannot obtain `&'static str` from `String` unless we use `Box::leak`
                ""
            }
        }
    }

    /// Returns the string equivalent of this auth scheme ID.
    pub fn inner(&self) -> &str {
        &self.scheme_id
    }
}

impl From<&'static str> for AuthSchemeId {
    fn from(scheme_id: &'static str) -> Self {
        Self::new(scheme_id)
    }
}

impl From<Cow<'static, str>> for AuthSchemeId {
    fn from(scheme_id: Cow<'static, str>) -> Self {
        Self { scheme_id }
    }
}

impl PartialEq for AuthSchemeId {
    fn eq(&self, other: &Self) -> bool {
        let self_normalized = normalize_auth_scheme_id(&self.scheme_id);
        let other_normalized = normalize_auth_scheme_id(&other.scheme_id);
        self_normalized == other_normalized
    }
}

impl std::hash::Hash for AuthSchemeId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash the normalized scheme ID to ensure equal `AuthSchemeId`s have equal hashes
        normalize_auth_scheme_id(&self.scheme_id).hash(state);
    }
}

impl Ord for AuthSchemeId {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_normalized = normalize_auth_scheme_id(&self.scheme_id);
        let other_normalized = normalize_auth_scheme_id(&other.scheme_id);
        self_normalized.cmp(other_normalized)
    }
}

impl PartialOrd for AuthSchemeId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Normalizes auth scheme IDs for comparison and hashing by treating "no_auth" and "noAuth" as equivalent
// by converting "no_auth" to "noAuth".
// This is for backward compatibility; "no_auth" was incorrectly used in pre-GA versions of the SDK and
// could be used still in some places.
fn normalize_auth_scheme_id(scheme_id: &str) -> &str {
    if scheme_id == "no_auth" {
        "noAuth"
    } else {
        scheme_id
    }
}

/// Parameters needed to resolve auth scheme options.
///
/// Most generated clients will use the [`StaticAuthSchemeOptionResolver`](static_resolver::StaticAuthSchemeOptionResolver),
/// which doesn't require any parameters for resolution (and has its own empty params struct).
///
/// However, more complex auth scheme resolvers may need modeled parameters in order to resolve
/// the auth scheme options. For those, this params struct holds a type erased box so that any
/// kind of parameters can be contained within, and type casted by the auth scheme option resolver
/// implementation.
#[derive(Debug)]
pub struct AuthSchemeOptionResolverParams(TypeErasedBox);

impl AuthSchemeOptionResolverParams {
    /// Creates a new [`AuthSchemeOptionResolverParams`].
    pub fn new<T: fmt::Debug + Send + Sync + 'static>(params: T) -> Self {
        Self(TypeErasedBox::new(params))
    }

    /// Returns the underlying parameters as the type `T` if they are that type.
    pub fn get<T: fmt::Debug + Send + Sync + 'static>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}

impl Storable for AuthSchemeOptionResolverParams {
    type Storer = StoreReplace<Self>;
}

new_type_future! {
    #[doc = "Future for [`ResolveAuthSchemeOptions::resolve_auth_scheme_options_v2`]."]
    pub struct AuthSchemeOptionsFuture<'a, Vec<AuthSchemeOption>, BoxError>;
}

/// Resolver for auth scheme options.
///
/// The orchestrator needs to select an auth scheme to sign requests with, and potentially
/// from several different available auth schemes. Smithy models have a number of ways
/// to specify which operations can use which auth schemes under which conditions, as
/// documented in the [Smithy spec](https://smithy.io/2.0/spec/authentication-traits.html).
///
/// The orchestrator uses the auth scheme option resolver runtime component to resolve
/// an ordered list of options that are available to choose from for a given request.
/// This resolver can be a simple static list, such as with the
/// [`StaticAuthSchemeOptionResolver`](static_resolver::StaticAuthSchemeOptionResolver),
/// or it can be a complex code generated resolver that incorporates parameters from both
/// the model and the resolved endpoint.
pub trait ResolveAuthSchemeOptions: Send + Sync + fmt::Debug {
    #[deprecated(
        note = "This method is deprecated, use `resolve_auth_scheme_options_v2` instead.",
        since = "1.8.0"
    )]
    /// Returns a list of available auth scheme options to choose from.
    fn resolve_auth_scheme_options(
        &self,
        _params: &AuthSchemeOptionResolverParams,
    ) -> Result<Cow<'_, [AuthSchemeId]>, BoxError> {
        unimplemented!("This method is deprecated, use `resolve_auth_scheme_options_v2` instead.");
    }

    #[allow(deprecated)]
    /// Returns a list of available auth scheme options to choose from.
    fn resolve_auth_scheme_options_v2<'a>(
        &'a self,
        params: &'a AuthSchemeOptionResolverParams,
        _cfg: &'a ConfigBag,
        _runtime_components: &'a RuntimeComponents,
    ) -> AuthSchemeOptionsFuture<'a> {
        AuthSchemeOptionsFuture::ready({
            self.resolve_auth_scheme_options(params).map(|options| {
                options
                    .iter()
                    .cloned()
                    .map(|scheme_id| {
                        AuthSchemeOption::builder()
                            .scheme_id(scheme_id)
                            .build()
                            .expect("required fields set")
                    })
                    .collect::<Vec<_>>()
            })
        })
    }
}

/// A shared auth scheme option resolver.
#[derive(Clone, Debug)]
pub struct SharedAuthSchemeOptionResolver(Arc<dyn ResolveAuthSchemeOptions>);

impl SharedAuthSchemeOptionResolver {
    /// Creates a new [`SharedAuthSchemeOptionResolver`].
    pub fn new(auth_scheme_option_resolver: impl ResolveAuthSchemeOptions + 'static) -> Self {
        Self(Arc::new(auth_scheme_option_resolver))
    }
}

impl ResolveAuthSchemeOptions for SharedAuthSchemeOptionResolver {
    #[allow(deprecated)]
    fn resolve_auth_scheme_options(
        &self,
        params: &AuthSchemeOptionResolverParams,
    ) -> Result<Cow<'_, [AuthSchemeId]>, BoxError> {
        (*self.0).resolve_auth_scheme_options(params)
    }

    fn resolve_auth_scheme_options_v2<'a>(
        &'a self,
        params: &'a AuthSchemeOptionResolverParams,
        cfg: &'a ConfigBag,
        runtime_components: &'a RuntimeComponents,
    ) -> AuthSchemeOptionsFuture<'a> {
        (*self.0).resolve_auth_scheme_options_v2(params, cfg, runtime_components)
    }
}

impl_shared_conversions!(
    convert SharedAuthSchemeOptionResolver
    from ResolveAuthSchemeOptions
    using SharedAuthSchemeOptionResolver::new
);

/// An auth scheme.
///
/// Auth schemes have unique identifiers (the `scheme_id`),
/// and provide an identity resolver and a signer.
pub trait AuthScheme: Send + Sync + fmt::Debug {
    /// Returns the unique identifier associated with this auth scheme.
    ///
    /// This identifier is used to refer to this auth scheme from the
    /// [`ResolveAuthSchemeOptions`], and is also associated with
    /// identity resolvers in the config.
    fn scheme_id(&self) -> AuthSchemeId;

    /// Returns the identity resolver that can resolve an identity for this scheme, if one is available.
    ///
    /// The [`AuthScheme`] doesn't actually own an identity resolver. Rather, identity resolvers
    /// are configured as runtime components. The auth scheme merely chooses a compatible identity
    /// resolver from the runtime components via the [`GetIdentityResolver`] trait. The trait is
    /// given rather than the full set of runtime components to prevent complex resolution logic
    /// involving multiple components from taking place in this function, since that's not the
    /// intended use of this design.
    fn identity_resolver(
        &self,
        identity_resolvers: &dyn GetIdentityResolver,
    ) -> Option<SharedIdentityResolver>;

    /// Returns the signing implementation for this auth scheme.
    fn signer(&self) -> &dyn Sign;
}

/// Container for a shared auth scheme implementation.
#[derive(Clone, Debug)]
pub struct SharedAuthScheme(Arc<dyn AuthScheme>);

impl SharedAuthScheme {
    /// Creates a new [`SharedAuthScheme`] from the given auth scheme.
    pub fn new(auth_scheme: impl AuthScheme + 'static) -> Self {
        Self(Arc::new(auth_scheme))
    }
}

impl AuthScheme for SharedAuthScheme {
    fn scheme_id(&self) -> AuthSchemeId {
        self.0.scheme_id()
    }

    fn identity_resolver(
        &self,
        identity_resolvers: &dyn GetIdentityResolver,
    ) -> Option<SharedIdentityResolver> {
        self.0.identity_resolver(identity_resolvers)
    }

    fn signer(&self) -> &dyn Sign {
        self.0.signer()
    }
}

impl ValidateConfig for SharedAuthScheme {}

impl_shared_conversions!(convert SharedAuthScheme from AuthScheme using SharedAuthScheme::new);

/// Signing implementation for an auth scheme.
pub trait Sign: Send + Sync + fmt::Debug {
    /// Sign the given request with the given identity, components, and config.
    ///
    /// If the provided identity is incompatible with this signer, an error must be returned.
    fn sign_http_request(
        &self,
        request: &mut HttpRequest,
        identity: &Identity,
        auth_scheme_endpoint_config: AuthSchemeEndpointConfig<'_>,
        runtime_components: &RuntimeComponents,
        config_bag: &ConfigBag,
    ) -> Result<(), BoxError>;
}

/// Endpoint configuration for the selected auth scheme.
///
/// The configuration held by this struct originates from the endpoint rule set in the service model.
///
/// This struct gets added to the request state by the auth orchestrator.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct AuthSchemeEndpointConfig<'a>(Option<&'a Document>);

impl<'a> AuthSchemeEndpointConfig<'a> {
    /// Creates an empty [`AuthSchemeEndpointConfig`].
    pub fn empty() -> Self {
        Self(None)
    }

    /// Returns the endpoint configuration as a [`Document`].
    pub fn as_document(&self) -> Option<&'a Document> {
        self.0
    }
}

impl<'a> From<Option<&'a Document>> for AuthSchemeEndpointConfig<'a> {
    fn from(value: Option<&'a Document>) -> Self {
        Self(value)
    }
}

impl<'a> From<&'a Document> for AuthSchemeEndpointConfig<'a> {
    fn from(value: &'a Document) -> Self {
        Self(Some(value))
    }
}

/// An ordered list of [AuthSchemeId]s
///
/// Can be used to reorder already-resolved auth schemes by an auth scheme resolver.
/// This list is intended as a hint rather than a strict override;
/// any schemes not present in the resolved auth schemes will be ignored.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AuthSchemePreference {
    preference_list: Vec<AuthSchemeId>,
}

impl Storable for AuthSchemePreference {
    type Storer = StoreReplace<Self>;
}

impl IntoIterator for AuthSchemePreference {
    type Item = AuthSchemeId;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.preference_list.into_iter()
    }
}

impl<T> From<T> for AuthSchemePreference
where
    T: AsRef<[AuthSchemeId]>,
{
    fn from(slice: T) -> Self {
        AuthSchemePreference {
            preference_list: slice.as_ref().to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_scheme_id_equality_no_auth_variants() {
        let no_auth_legacy = AuthSchemeId::new("no_auth");
        let no_auth_camel = AuthSchemeId::new("noAuth");

        // Test that "no_auth" and "noAuth" are considered equal
        assert_eq!(no_auth_legacy, no_auth_camel);
        assert_eq!(no_auth_camel, no_auth_legacy);
    }

    #[test]
    fn test_auth_scheme_id_equality_same_schemes() {
        let sigv4_1 = AuthSchemeId::new("sigv4");
        let sigv4_2 = AuthSchemeId::new("sigv4");

        // Test that identical schemes are equal
        assert_eq!(sigv4_1, sigv4_2);
    }

    #[test]
    fn test_auth_scheme_id_inequality_different_schemes() {
        let sigv4 = AuthSchemeId::new("sigv4");
        let sigv4a = AuthSchemeId::new("sigv4a");
        let bearer = AuthSchemeId::new("httpBearerAuth");

        // Test that different schemes are not equal
        assert_ne!(sigv4, sigv4a);
        assert_ne!(sigv4, bearer);
        assert_ne!(sigv4a, bearer);
    }

    #[test]
    fn test_auth_scheme_id_no_auth_vs_other_schemes() {
        let no_auth_legacy = AuthSchemeId::new("no_auth");
        let no_auth_camel = AuthSchemeId::new("noAuth");
        let sigv4 = AuthSchemeId::new("sigv4");
        let bearer = AuthSchemeId::new("httpBearerAuth");

        // Test that no_auth variants are not equal to other schemes
        assert_ne!(no_auth_legacy, sigv4);
        assert_ne!(no_auth_camel, sigv4);
        assert_ne!(no_auth_legacy, bearer);
        assert_ne!(no_auth_camel, bearer);

        // Test symmetry
        assert_ne!(sigv4, no_auth_legacy);
        assert_ne!(sigv4, no_auth_camel);
        assert_ne!(bearer, no_auth_legacy);
        assert_ne!(bearer, no_auth_camel);
    }

    #[test]
    fn test_normalize_auth_scheme_id_function() {
        // Test the helper function directly
        assert_eq!("noAuth", normalize_auth_scheme_id("no_auth"));
        assert_eq!("noAuth", normalize_auth_scheme_id("noAuth"));
        assert_eq!("sigv4", normalize_auth_scheme_id("sigv4"));
        assert_eq!("httpBearerAuth", normalize_auth_scheme_id("httpBearerAuth"));
        assert_eq!("custom_scheme", normalize_auth_scheme_id("custom_scheme"));
    }

    #[test]
    fn test_auth_scheme_id_reflexivity() {
        let no_auth_legacy = AuthSchemeId::new("no_auth");
        let no_auth_camel = AuthSchemeId::new("noAuth");
        let sigv4 = AuthSchemeId::new("sigv4");

        // Test reflexivity: x == x
        assert_eq!(no_auth_legacy, no_auth_legacy);
        assert_eq!(no_auth_camel, no_auth_camel);
        assert_eq!(sigv4, sigv4);
    }

    #[test]
    fn test_auth_scheme_id_transitivity() {
        let no_auth_1 = AuthSchemeId::new("no_auth");
        let no_auth_2 = AuthSchemeId::new("noAuth");
        let no_auth_3 = AuthSchemeId::new("no_auth");

        // Test transitivity: if a == b and b == c, then a == c
        assert_eq!(no_auth_1, no_auth_2);
        assert_eq!(no_auth_2, no_auth_3);
        assert_eq!(no_auth_1, no_auth_3);
    }

    #[test]
    fn test_auth_scheme_id_hash_consistency() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        fn calculate_hash<T: Hash>(t: &T) -> u64 {
            let mut s = DefaultHasher::new();
            t.hash(&mut s);
            s.finish()
        }

        // Test that equal AuthSchemeIds have the same hash
        let no_auth_legacy = AuthSchemeId::new("no_auth");
        let no_auth_camel = AuthSchemeId::new("noAuth");

        // Since these are equal, they must have the same hash
        assert_eq!(no_auth_legacy, no_auth_camel);
        assert_eq!(
            calculate_hash(&no_auth_legacy),
            calculate_hash(&no_auth_camel)
        );

        // Test that identical schemes have the same hash
        let sigv4_1 = AuthSchemeId::new("sigv4");
        let sigv4_2 = AuthSchemeId::new("sigv4");
        assert_eq!(calculate_hash(&sigv4_1), calculate_hash(&sigv4_2));

        // Test that different schemes have different hashes (highly likely but not guaranteed)
        let sigv4 = AuthSchemeId::new("sigv4");
        let sigv4a = AuthSchemeId::new("sigv4a");
        let bearer = AuthSchemeId::new("httpBearerAuth");

        // These should be different (though hash collisions are theoretically possible)
        assert_ne!(calculate_hash(&sigv4), calculate_hash(&sigv4a));
        assert_ne!(calculate_hash(&sigv4), calculate_hash(&bearer));
        assert_ne!(calculate_hash(&sigv4a), calculate_hash(&bearer));
    }

    #[test]
    fn test_auth_scheme_id_hash_in_collections() {
        use std::collections::{HashMap, HashSet};

        let no_auth_legacy = AuthSchemeId::new("no_auth");
        let no_auth_camel = AuthSchemeId::new("noAuth");
        let sigv4 = AuthSchemeId::new("sigv4");
        let bearer = AuthSchemeId::new("httpBearerAuth");

        // Test HashSet behavior - equal items should be treated as the same
        let mut set = HashSet::new();
        set.insert(no_auth_legacy.clone());
        set.insert(no_auth_camel.clone());
        set.insert(sigv4.clone());
        set.insert(bearer.clone());

        // Should only have 3 items since no_auth_legacy and no_auth_camel are equal
        assert_eq!(set.len(), 3);
        assert!(set.contains(&no_auth_legacy));
        assert!(set.contains(&no_auth_camel));
        assert!(set.contains(&sigv4));
        assert!(set.contains(&bearer));

        // Test HashMap behavior
        let mut map = HashMap::new();
        map.insert(no_auth_legacy.clone(), "legacy");
        map.insert(no_auth_camel.clone(), "camel");
        map.insert(sigv4.clone(), "v4");

        // Should only have 2 entries since no_auth_legacy and no_auth_camel are equal
        assert_eq!(map.len(), 2);
        // The value should be "camel" since it was inserted last
        assert_eq!(map.get(&no_auth_legacy), Some(&"camel"));
        assert_eq!(map.get(&no_auth_camel), Some(&"camel"));
        assert_eq!(map.get(&sigv4), Some(&"v4"));
    }

    #[test]
    fn test_auth_scheme_id_ord_consistency() {
        let no_auth_legacy = AuthSchemeId::new("no_auth");
        let no_auth_camel = AuthSchemeId::new("noAuth");
        let sigv4 = AuthSchemeId::new("sigv4");
        let sigv4a = AuthSchemeId::new("sigv4a");
        let bearer = AuthSchemeId::new("httpBearerAuth");

        // Test that equal items compare as equal
        assert_eq!(no_auth_legacy.cmp(&no_auth_camel), Ordering::Equal);
        assert_eq!(no_auth_camel.cmp(&no_auth_legacy), Ordering::Equal);

        // Test reflexivity: x.cmp(&x) == Equal
        assert_eq!(sigv4.cmp(&sigv4), Ordering::Equal);
        assert_eq!(bearer.cmp(&bearer), Ordering::Equal);

        // Test that ordering is consistent with string ordering of normalized values
        // "sigv4" < "sigv4a" lexicographically
        assert_eq!(sigv4.cmp(&sigv4a), Ordering::Less);
        assert_eq!(sigv4a.cmp(&sigv4), Ordering::Greater);

        // Test transitivity with a chain of comparisons
        let schemes = vec![
            AuthSchemeId::new("a_scheme"),
            AuthSchemeId::new("b_scheme"),
            AuthSchemeId::new("c_scheme"),
        ];

        // a < b < c should hold
        assert_eq!(schemes[0].cmp(&schemes[1]), Ordering::Less);
        assert_eq!(schemes[1].cmp(&schemes[2]), Ordering::Less);
        assert_eq!(schemes[0].cmp(&schemes[2]), Ordering::Less);
    }

    #[test]
    fn test_auth_scheme_id_ord_sorting() {
        let mut schemes = vec![
            AuthSchemeId::new("z_last"),
            AuthSchemeId::new("no_auth"), // Should be normalized to "noAuth"
            AuthSchemeId::new("sigv4a"),
            AuthSchemeId::new("noAuth"),
            AuthSchemeId::new("sigv4"),
            AuthSchemeId::new("a_first"),
        ];

        schemes.sort();
        dbg!(&schemes);

        // Expected order after sorting (considering normalization):
        // "a_first", "sigv4", "sigv4a", "no_auth", "noAuth", "z_last"
        // Note: "no_auth" gets normalized to "noAuth" for comparison
        let expected_inner_values =
            vec!["a_first", "no_auth", "noAuth", "sigv4", "sigv4a", "z_last"];

        assert_eq!(schemes.len(), expected_inner_values.len());
        for (scheme, expected) in schemes.iter().zip(expected_inner_values.iter()) {
            assert_eq!(scheme.inner(), *expected);
        }
    }

    #[test]
    fn test_auth_scheme_id_ord_with_cow_variants() {
        use std::borrow::Cow;

        // Test ordering with different Cow variants
        let borrowed = AuthSchemeId::new("test_scheme");
        let owned = AuthSchemeId::from(Cow::Owned("test_scheme".to_string()));
        let borrowed2 = AuthSchemeId::from(Cow::Borrowed("test_scheme"));

        // All should be equal
        assert_eq!(borrowed, owned);
        assert_eq!(borrowed, borrowed2);
        assert_eq!(owned, borrowed2);

        // All should have the same ordering
        assert_eq!(borrowed.cmp(&owned), Ordering::Equal);
        assert_eq!(borrowed.cmp(&borrowed2), Ordering::Equal);
        assert_eq!(owned.cmp(&borrowed2), Ordering::Equal);

        // Test with different values
        let borrowed_a = AuthSchemeId::new("a_scheme");
        let owned_b = AuthSchemeId::from(Cow::Owned("b_scheme".to_string()));

        assert_eq!(borrowed_a.cmp(&owned_b), Ordering::Less);
        assert_eq!(owned_b.cmp(&borrowed_a), Ordering::Greater);
    }
}
