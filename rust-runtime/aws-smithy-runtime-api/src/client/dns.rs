/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Interfaces for resolving DNS

use crate::box_error::BoxError;
use crate::impl_shared_conversions;
use std::error::Error as StdError;
use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

/// Error that occurs when failing to perform a DNS lookup.
#[derive(Debug)]
pub struct ResolveDnsError {
    source: BoxError,
}

impl ResolveDnsError {
    /// Creates a new `DnsLookupFailed` error.
    pub fn new(source: impl Into<BoxError>) -> Self {
        ResolveDnsError {
            source: source.into(),
        }
    }
}

impl fmt::Display for ResolveDnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to perform DNS lookup")
    }
}

impl StdError for ResolveDnsError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&*self.source as _)
    }
}

new_type_future! {
    #[doc = "New-type for the future returned by the [`ResolveDns`] trait."]
    pub struct DnsFuture<'a, Vec<IpAddr>, ResolveDnsError>;
}

/////////////////////////////

/// Result of performing a DNS lookup
pub type ResolveDnsResult = Result<Vec<IpAddr>, ResolveDnsError>;

/// Models a name resolution request.
/// Future possibilities could include TTL override, IPv4 vs IPv6 restrictions, etc...
/// This is the request type used in the public resolution API.
#[derive(Debug, Clone)]
pub struct ResolveRequest {
    name: String,
    ttl: Option<Duration>,
    refresh_delay: Option<Duration>,
}

impl ResolveRequest {
    /// Create a new request builder instance
    pub fn builder(name: &str) -> ResolveRequestBuilder {
        ResolveRequestBuilder {
            request: ResolveRequest {
                name: name.to_string(),
                ttl: None,
                refresh_delay: None,
            },
        }
    }

    /// the host name to perform a name resolution for
    pub fn name(&self) -> &str {
        &self.name
    }

    /// optional per-request TTL override for a resolved address.  Only lookups
    /// triggered by this request will have the override TTL applied.  If not set, the resolver's
    /// TTL will be used.
    pub fn ttl(&self) -> Option<Duration> {
        self.ttl
    }

    /// optional per-request override for the minimum delay between low-level resolution attempts.
    /// If specified, takes precedence over the resolver's configuration.
    pub fn refresh_delay(&self) -> Option<Duration> {
        self.refresh_delay
    }
}

/// Builder type for ResolveRequest instances
#[derive(Debug)]
pub struct ResolveRequestBuilder {
    request: ResolveRequest,
}

impl ResolveRequestBuilder {
    /// Create a new ResolveRequest instance
    pub fn build(&self) -> ResolveRequest {
        self.request.clone()
    }

    /// Overrides the resolver's default address TTL value with a different duration.  Only lookups
    /// triggered by this request will have the override TTL applied.
    pub fn with_ttl(&mut self, ttl: Duration) -> &mut Self {
        self.request.ttl = Some(ttl);
        self
    }

    /// Overrides the resolver's minimum delay between low-level resolution attempts.
    pub fn with_refresh_delay(&mut self, refresh_delay: Duration) -> &mut Self {
        self.request.refresh_delay = Some(refresh_delay);
        self
    }
}

/// Determines how the address is penalized with respect to future resolution requests.
#[non_exhaustive]
#[derive(Debug, Eq, PartialEq)]
pub enum AddressPenaltyCategory {
    /// Never use this address again for as long as it's tracked
    Fatal,

    /// Avoid this address unless there's no other alternative
    Major,

    /// Avoid using this address for at least a configurable amount of time
    Custom(Duration),
}

/// Event used to discourage or route away from an associated IP address.
#[derive(Debug)]
pub struct HostEventAddressPenalty {
    category: AddressPenaltyCategory,

    source: Option<BoxError>,
}

impl HostEventAddressPenalty {
    /// Creates a new builder for an address penalty host event variant
    pub fn builder(category: AddressPenaltyCategory) -> HostEventAddressPenaltyBuilder {
        HostEventAddressPenaltyBuilder {
            penalty: HostEventAddressPenalty {
                category,
                source: None,
            },
        }
    }

    /// How the resolver should penalize the associated address in future resolutions
    pub fn category(&self) -> &AddressPenaltyCategory {
        &self.category
    }

    /// Actual source error triggering this penalty
    pub fn source(&self) -> Option<&BoxError> {
        self.source.as_ref()
    }
}

/// Builder type for HostEventAddressPenalty instances
#[derive(Debug)]
pub struct HostEventAddressPenaltyBuilder {
    penalty: HostEventAddressPenalty,
}

impl HostEventAddressPenaltyBuilder {
    /// Sets the source error that triggered the address penalty
    pub fn with_source(&mut self, error: BoxError) -> &mut HostEventAddressPenaltyBuilder {
        self.penalty.source = Some(error);
        self
    }

    /// (Consuming) builds a new HostEventAddressPenalty instance
    pub fn build(self) -> HostEventAddressPenalty {
        self.penalty
    }
}

/// Per-host-event-type variants
#[non_exhaustive]
#[derive(Debug)]
pub enum HostEventInformation {
    /// Notifies the resolver that a penalty should be applied to a resolved address for the
    /// purposes of connection assignment
    AddressPenalty(HostEventAddressPenalty),
}

/// A connection-related event that could cause us to update our internal bookkeeping that
/// helps distribute IPs across connections.
#[derive(Debug)]
pub struct HostEvent {
    /// Name of the host that the address was resolver for
    name: String,

    /// IP address of the connection that encountered an error
    address: IpAddr,

    /// Per-event-type data
    event_data: HostEventInformation,
}

impl HostEvent {
    /// Creates a new builder for HostEvents
    pub fn builder(
        name: &str,
        address: IpAddr,
        event_data: HostEventInformation,
    ) -> HostEventBuilder {
        HostEventBuilder {
            event: HostEvent {
                name: name.to_string(),
                address,
                event_data,
            },
        }
    }

    /// Host name this event corresponds to
    pub fn name(&self) -> &str {
        &self.name
    }

    /// IP address this event corresponds to
    pub fn address(&self) -> IpAddr {
        self.address
    }

    /// Host event variant data
    pub fn event_data(&self) -> &HostEventInformation {
        &self.event_data
    }
}

/// Builder type for HostEvent instances
#[derive(Debug)]
pub struct HostEventBuilder {
    event: HostEvent,
}

impl HostEventBuilder {
    /// Creates a new HostEvent instance
    pub fn build(self) -> HostEvent {
        self.event
    }
}

/// Unit result type for our public API(s)
pub type ResolverResult = Result<(), BoxError>;

/////////////////////////////

/// Trait for resolving domain names
pub trait ResolveDns: fmt::Debug + Send + Sync {
    /// Asynchronously resolve the given domain name
    fn resolve_dns<'a>(&'a self, name: &'a str) -> DnsFuture<'a>;

    /// Asynchronously resolve the given domain name, with extensibility
    fn resolve<'a>(&'a self, request: ResolveRequest) -> DnsFuture<'a> {
        DnsFuture::new(async move { self.resolve_dns(request.name()).await })
    }

    /// Notify the resolver of an external host-related event that may affect how it
    /// chooses to distribute IP addresses to resolution requests
    fn notify(&self, _event: HostEvent) -> ResolverResult {
        Ok(()) // ToDo: change to an error?
    }
}

/// Shared instance of [`ResolveDns`].
#[derive(Clone, Debug)]
pub struct SharedDnsResolver(Arc<dyn ResolveDns>);

impl SharedDnsResolver {
    /// Create a new `SharedDnsResolver`.
    pub fn new(resolver: impl ResolveDns + 'static) -> Self {
        Self(Arc::new(resolver))
    }
}

impl ResolveDns for SharedDnsResolver {
    fn resolve_dns<'a>(&'a self, name: &'a str) -> DnsFuture<'a> {
        self.0.resolve_dns(name)
    }

    fn resolve<'a>(&'a self, request: ResolveRequest) -> DnsFuture<'a> {
        self.0.resolve(request)
    }

    fn notify(&self, event: HostEvent) -> ResolverResult {
        self.0.notify(event)
    }
}

impl_shared_conversions!(convert SharedDnsResolver from ResolveDns using SharedDnsResolver::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_send() {
        fn is_send<T: Send>() {}
        is_send::<DnsFuture<'_>>();
    }
}
