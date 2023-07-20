Identity and Auth in Clients
============================

The [Smithy specification establishes several auth related modeling traits] that can be applied to
operation and service shapes. To briefly summarize:

- The auth schemes that are supported by a service are declared on the service shape
- Operation shapes MAY specify the subset of service-defined auth schemes they support. If none are specified, then all service-defined auth schemes are supported.

A smithy code generator MUST support at least one auth scheme for every modeled operation, but it need not support ALL modeled auth schemes.

This design document establishes how smithy-rs implements this specification.

Terminology
-----------

- **Auth:** Either a shorthand that represents both of the authentication and authorization terms below,
or an ambiguous representation of one of them. In this doc, this term will always refer to both.
- **Authentication:** The process of proving an entity is who they claim they are, sometimes referred to as AuthN.
- **Authorization:** The process of granting an authenticated entity the permission to
do something, sometimes referred to as AuthZ.
- **Identity:** The information required for authentication.
- **Signing:** The process of attaching metadata to a request that allows a server to authenticate that request.

Overview of Smithy Client Auth
------------------------------

There are two stages to identity and auth:
1. Configuration
2. Execution

### The configuration stage

First, let's establish the aspects of auth that can be configured from the model at codegen time.

- **Data**
    - **AuthSchemeOptionResolverParams:** parameters required to resolve auth scheme options. These parameters are allowed
      to come from both the client config and the operation input structs.
    - **AuthSchemes:** a list of auth schemes that can be used to sign HTTP requests. This information
      comes directly from the service model.
    - **AuthSchemeProperties:** configuration from the auth scheme for the signer.
    - **IdentityResolvers:** list of available identity resolvers.
- **Implementations**
    - **IdentityResolver:** resolves an identity for use in authentication.
      There can be multiple identity resolvers that need to be selected from.
    - **Signer:** a signing implementation that signs a HTTP request.
    - **AuthSchemeOptionResolver:** resolves a list of auth scheme options for a given operation and its inputs.

As it is undocumented (at time of writing), this document assumes that the code generator
creates one service-level runtime plugin, and an operation-level runtime plugin per operation, hence
referred to as the service runtime plugin and operation runtime plugin.

The code generator emits code to add identity resolvers and HTTP auth schemes to the config bag
in the service runtime plugin. It then emits code to register an interceptor in the operation runtime
plugin that reads the operation input to generate the auth scheme option resolver params (which also get added
to the config bag).

### The execution stage

At a high-level, the process of resolving an identity and signing a request looks as follows:

1. Retrieve the `AuthSchemeOptionResolverParams` from the config bag. The `AuthSchemeOptionResolverParams` allow client
config and operation inputs to play a role in which auth scheme option is selected.
2. Retrieve the `AuthSchemeOptionResolver` from the config bag, and use it to resolve the auth scheme options available
with the `AuthSchemeOptionResolverParams`. The returned auth scheme options are in priority order.
3. Retrieve the `IdentityResolvers` list from the config bag.
4. For each auth scheme option:
   1. Attempt to find an HTTP auth scheme for that auth scheme option in the config bag (from the `AuthSchemes` list).
   2. If an auth scheme is found:
      1. Use the auth scheme to extract the correct identity resolver from the `IdentityResolvers` list.
      2. Retrieve the `Signer` implementation from the auth scheme.
      3. Use the `IdentityResolver` to resolve the identity needed for signing.
      4. Sign the request with the identity, and break out of the loop from step #4.

In general, it is assumed that if an HTTP auth scheme exists for an auth scheme option, then an identity resolver
also exists for that auth scheme option. Otherwise, the auth option was configured incorrectly during codegen.

How this looks in Rust
----------------------

The client will use trait objects and dynamic dispatch for the `IdentityResolver`,
`Signer`, and `AuthSchemeOptionResolver` implementations. Generics could potentially be used,
but the number of generic arguments and trait bounds in the orchestrator would balloon to
unmaintainable levels if each configurable implementation in it was made generic.

These traits look like this:

```rust,ignore
#[derive(Clone, Debug)]
pub struct AuthSchemeId {
    scheme_id: &'static str,
}

pub trait AuthSchemeOptionResolver: Send + Sync + Debug {
    fn resolve_auth_scheme_options<'a>(
        &'a self,
        params: &AuthSchemeOptionResolverParams,
    ) -> Result<Cow<'a, [AuthSchemeId]>, BoxError>;
}

pub trait IdentityResolver: Send + Sync + Debug {
    fn resolve_identity(&self, config: &ConfigBag) -> BoxFallibleFut<Identity>;
}

pub trait Signer: Send + Sync + Debug {
    /// Return a signed version of the given request using the given identity.
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
```

`IdentityResolver` and `Signer` implementations are both given an `Identity`, but
will need to understand what the concrete data type underlying that identity is. The `Identity` struct
uses a `Arc<dyn Any>` to represent the actual identity data so that generics are not needed in
the traits:

```rust,ignore
#[derive(Clone, Debug)]
pub struct Identity {
    data: Arc<dyn Any + Send + Sync>,
    expiration: Option<SystemTime>,
}
```

Identities can often be cached and reused across several requests, which is why the `Identity` uses `Arc`
rather than `Box`. This also reduces the allocations required. The signer implementations
will use downcasting to access the identity data types they understand. For example, with AWS SigV4,
it might look like the following:

```rust,ignore
fn sign_http_request(
    &self,
    request: &mut HttpRequest,
    identity: &Identity,
    auth_scheme_endpoint_config: AuthSchemeEndpointConfig<'_>,
    runtime_components: &RuntimeComponents,
    config_bag: &ConfigBag,
) -> Result<(), BoxError> {
    let aws_credentials = identity.data::<Credentials>()
        .ok_or_else(|| "The SigV4 signer requires AWS credentials")?;
    let access_key = &aws_credentials.secret_access_key;
    // -- snip --
}
```

Also note that identity data structs are expected to censor their own sensitive fields, as
`Identity` implements the automatically derived `Debug` trait.

### Challenges with this `Identity` design

A keen observer would note that there is an `expiration` field on `Identity`, and may ask, "what about
non-expiring identities?" This is the result of a limitation on `Box<dyn Any>`, where it can only be
downcasted to concrete types. There is no way to downcast to a `dyn Trait` since the information required
to verify that that type is that trait is lost at compile time (a `std::any::TypeId` only encodes information
about the concrete type).

In an ideal world, it would be possible to extract the expiration like this:
```rust,ignore
pub trait ExpiringIdentity {
    fn expiration(&self) -> SystemTime;
}

let identity: Identity = some_identity();
if let Some(expiration) = identity.data::<&dyn ExpiringIdentity>().map(ExpiringIdentity::expiration) {
    // make a decision based on that expiration
}
```

Theoretically, you should be able to save off additional type information alongside the `Box<dyn Any>` and use
unsafe code to transmute to known traits, but it is difficult to implement in practice, and adds unsafe code
in a security critical piece of code that could otherwise be avoided.

The `expiration` field is a special case that is allowed onto the `Identity` struct directly since identity
cache implementations will always need to be aware of this piece of information, and having it as an `Option`
still allows for non-expiring identities.

Ultimately, this design constrains `Signer` implementations to concrete types. There is no world
where an `Signer` can operate across multiple unknown identity data types via trait, and that
should be OK since the signer implementation can always be wrapped with an implementation that is aware
of the concrete type provided by the identity resolver, and can do any necessary conversions.

[Smithy specification establishes several auth related modeling traits]: https://smithy.io/2.0/spec/authentication-traits.html
