/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
use crate::cache::*;
use aws_smithy_runtime_api::client::dns::*;
use priority_queue::PriorityQueue;
use std::cmp::Reverse;
use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use std::hash::Hash;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::{Duration, Instant};

/*

A resolver that uses shared state to distribute IPs to resolution requests
from arbitrary sources.  This allows us to properly implement SRA-style name resolution which
has round-robin IP distribution shared across all threads/runtimes interested in establishing
connections to a given name.  This resolver can be shared across a transfer manager's runtime
threads, multiple SDK clients, etc...

Shared state via explicit mutual exclusion is a nightmare, so we use a channel-based
CSP architecture.

The resolver can only be interacted with via a communication channel that can contain resolution
requests, results, and more.  A lightweight recurrent async task loops on that channel's receiver
processing incoming operations.  This creates a natural "serialization" of all resolution
requests regardless of source/location, with an associated private shared state for bookkeeping.

Currently, the async processing task behaves as follows:

On a resolution request:
   If we have good IPs for that name, resolve the request with the least-used good IP
   If there are

      1. no good IPs for that name or
      2. it's been a while since we last resolved that name

      and no resolution is pending, kick off a resolution on the resolver's runtime.  If the
      request was not resolved earlier, park the request until a resolution returns a result.

On a resolution result:
   Process the list of IPs returned, adding new ones to the good IP set for that name and refreshing
     the TTL on existing ones.
   For all parked requests on that name, complete the request with a good IP (by least-used priority)
     or the error if the resolution failed.

On a host event:
   On connection error:
      If the error is fatal, remove the ip address from consideration permanently
      If the error is major, mark the ip address as unhealthy for a time period based on
        the TTL for lookups.  Unhealthy IPs cannot be used to answer resolve requests.  Unhealthy
        IPs can be prematurely promoted back to healthy if no healthy IPs exist.

On IP TTL expiration:
   Remove the ip from the resolver cache completely

IP TTL expiration (or promotion) is processed lazily and only when a successful resolve completes.
This means that in the event of an outage, the address cache will be preserved regardless of TTL.

 */

// it's useful to be able to override the resolver's notion of elapsed time when testing
type ClockFn = Arc<dyn Fn() -> Instant + Send + Sync>;

/// Determines what kind of addresses resolution should return.  Happy eyeballs is implemented
/// internally in hyper and so if we want to support it we need to return both ipv4 and ipv6
/// addresses (if they exist).  This makes error correlation much harder (impossible really)
/// because partial failures aren't detectable (for ex: fail to connect to ipv6 but succeed on
/// ipv4 will not trigger any detectable way of penalizing the ipv6 address).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum NameResolutionAddressTypes {
    /// Resolution should return one ipv4 and ipv6 address, if they exist
    #[default]
    All,

    /// Resolution should only return ipv4 addresses
    IpV4,

    /// Resolution should only return ipv6 addresses
    IpV6,
}

const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(60);

/// Refresh delay is the amount of time that needs to elapse between resolves on a given name.
/// Unlike the CRT, we don't "ping" resolves at this interval, instead we lazily do them over time
/// in response to lookup requests.
const DEFAULT_REFRESH_DELAY: Duration = Duration::from_secs(6);

/// Configuration options specific to an SRA Resolver
#[derive(Clone)]
pub struct SraResolverConfig {
    address_types: NameResolutionAddressTypes,

    resolution_ttl: Duration,

    refresh_delay: Duration,

    clock: ClockFn,
}

impl Default for SraResolverConfig {
    fn default() -> Self {
        Self {
            address_types: NameResolutionAddressTypes::default(),
            resolution_ttl: DEFAULT_CACHE_TTL,
            refresh_delay: DEFAULT_REFRESH_DELAY,
            #[allow(clippy::disallowed_methods)] // we're currently in a no-wasm module, so this is ok
            clock: Arc::new(Instant::now),
        }
    }
}

impl SraResolverConfig {
    /// Creates a new builder for SraResolverConfig instances
    pub fn builder() -> SraResolverConfigBuilder {
        SraResolverConfigBuilder {
            config: SraResolverConfig::default(),
        }
    }

    /// What types of addresses to resolve
    pub fn address_types(&self) -> NameResolutionAddressTypes {
        self.address_types
    }

    /// What TTL value to use for resolved addresses
    pub fn resolution_ttl(&self) -> Duration {
        self.resolution_ttl
    }

    /// Amount of time that needs to elapse between resolves for a given name
    pub fn refresh_delay(&self) -> Duration {
        self.refresh_delay
    }

    /// Clock function to use when computing time stamps
    pub fn clock(&self) -> ClockFn {
        self.clock.clone()
    }
}

impl Debug for SraResolverConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut formatter = f.debug_struct("SraResolverConfig");
        formatter.field("address_types", &self.address_types);
        formatter.field("resolution_ttl", &self.resolution_ttl);
        formatter.finish()
    }
}

/// Builder type for SraResolverConfig instances
#[derive(Debug)]
pub struct SraResolverConfigBuilder {
    config: SraResolverConfig,
}

impl SraResolverConfigBuilder {
    /// Configures what kind of addresses resolution should return
    pub fn with_address_types(&mut self, address_types: NameResolutionAddressTypes) -> &mut Self {
        self.config.address_types = address_types;
        self
    }

    /// Configures the time-to-live for each resolved address
    pub fn with_resolution_ttl(&mut self, ttl: Duration) -> &mut Self {
        self.config.resolution_ttl = ttl;
        self
    }

    /// Configures the per-name delay between lookups
    pub fn with_refresh_delay(&mut self, refresh_delay: Duration) -> &mut Self {
        self.config.refresh_delay = refresh_delay;
        self
    }

    /// (Test-only) Clock function to use when computing time stamps
    #[cfg(test)]
    pub fn with_clock(&mut self, clock: ClockFn) -> &mut Self {
        self.config.clock = clock.clone();
        self
    }

    /// Creates a new SraResolverConfig instance
    pub fn build(self) -> SraResolverConfig {
        self.config.clone()
    }
}

/// Tracks the cached state of IP addresses of a specific type.  Addresses are classified as
/// healthy (can be used) or unhealthy (temporarily not allowed to be used).  Healthy addresses
/// are distributed in a round-robin "generational" fashion.  When an address is first seen, it
/// is placed at generation 0, which puts it at the front of the queue.  When an address gets
/// mapped to a resolve request, its generation is set to the current value.  When all addresses
/// in the set are at the current generation value, we increment the current generation value.
///
/// This unconventional counting scheme is primarily due to the fact that we currently cannot tell
/// how many outstanding, in-use connections are using a given IP because the consumers (http
/// clients) of the resolver do not expose visibility into that at the moment.  If we knew how
/// many open connections were using a given IP, we would sort on that instead of "generation"
struct GenerationalAddressSet<T>
where
    T: Hash + Eq + Clone,
{
    /// A marker value that represents the approximate number of times we've cycled through
    /// the set of healthy addresses when distributing IPs.
    current_generation: u32,

    /// Priority queue of IpAddress structs ordered by the generation they were last acquired in
    healthy_addresses: PriorityQueue<T, Reverse<u32>>,

    /// Priority queue of IpAddress structs ordered by the timepoint that the address should
    /// move from unhealthy back to healthy
    unhealthy_addresses_by_expiration: PriorityQueue<T, Reverse<Instant>>,
}

impl<T> GenerationalAddressSet<T>
where
    T: Hash + Eq + Clone,
{
    /// Creates a new address set
    fn new() -> Self {
        GenerationalAddressSet {
            current_generation: 1,
            healthy_addresses: PriorityQueue::new(),
            unhealthy_addresses_by_expiration: PriorityQueue::new(),
        }
    }

    /// Is an address already in the set?
    fn contains(&self, value: &T) -> bool {
        self.healthy_addresses.contains(value)
            || self.unhealthy_addresses_by_expiration.contains(value)
    }

    /// Is the set empty?
    fn empty(&self) -> bool {
        self.healthy_addresses.is_empty() && self.unhealthy_addresses_by_expiration.is_empty()
    }

    /// Add an address to the set by inserting it into the healthy subset.
    fn add_address(&mut self, value: T) {
        if !self.contains(&value) {
            self.healthy_addresses.push(value, Reverse(0));
        }
    }

    /// Removes an address from the set
    fn remove_address(&mut self, value: &T) {
        self.healthy_addresses.remove(value);
        self.unhealthy_addresses_by_expiration.remove(value);
    }

    /// Returns an address from the set to be appended into a DNS resolution result
    fn acquire_address(&mut self, now: Instant) -> Option<T> {
        if self.empty() {
            return None;
        }

        // promote any addresses whose demotion has expired
        while !self.unhealthy_addresses_by_expiration.is_empty() {
            if self.unhealthy_addresses_by_expiration.peek().unwrap().1 .0 <= now {
                let promoted_addr = self.unhealthy_addresses_by_expiration.pop().unwrap().0;
                self.healthy_addresses.push(promoted_addr, Reverse(0));
            } else {
                break;
            }
        }

        // force promote an address if no healthy address exists.  We know in this case an unhealthy
        // address exists because we previously checked if we were empty.
        if self.healthy_addresses.is_empty() {
            let promoted_addr = self.unhealthy_addresses_by_expiration.pop().unwrap().0;
            self.healthy_addresses.push(promoted_addr, Reverse(0));
        }

        // guaranteed to have a healthy address at this point
        let (item, generation) = self.healthy_addresses.pop().unwrap();

        // if the best address's generation is current, we've acquired every healthy address in the
        // set while this generation is active, so increment the generation.
        if generation.0 >= self.current_generation {
            self.current_generation += 1;
        }

        // push the address back on the set using the current generation value
        self.healthy_addresses
            .push(item.clone(), Reverse(self.current_generation));

        Some(item)
    }

    /// Moves an address from a healthy to unhealthy state
    fn demote_address(&mut self, address: T, duration: Duration, now: Instant) {
        self.healthy_addresses.remove(&address);

        let expiration = now + duration;
        self.unhealthy_addresses_by_expiration
            .push(address, Reverse(expiration));
    }

    #[cfg(test)]
    fn current_generation(&self) -> u32 {
        self.current_generation
    }

    #[cfg(test)]
    fn get_healthy_data(&self, address: T) -> Option<u32> {
        if self.healthy_addresses.contains(&address) {
            Some(self.healthy_addresses.get(&address).unwrap().1 .0)
        } else {
            None
        }
    }

    #[cfg(test)]
    fn get_unhealthy_timestamp(&self, address: T) -> Option<Instant> {
        if self.unhealthy_addresses_by_expiration.contains(&address) {
            Some(
                self.unhealthy_addresses_by_expiration
                    .get(&address)
                    .unwrap()
                    .1
                     .0,
            )
        } else {
            None
        }
    }
}

/// Per-host-name struct that tracks resolved addresses, pending resolution requests, and more.
struct HostEntry {
    /// ordered queue of resolution requests that have not yet been completed
    pending_requests: VecDeque<ResolveRequestInternal>,

    /// known ipv4 addresses
    ipv4_addresses: GenerationalAddressSet<Ipv4Addr>,

    /// know ipv6 addresses
    ipv6_addresses: GenerationalAddressSet<Ipv6Addr>,

    /// all known addresses ordered by when we should forget them.  It is possible for an address
    /// to be in this set while not in either of the generational sets.  This is the fate of
    /// addresses which received a fatal connection error and prevents them from ever being used
    /// (even promoted from unhealthy to healthy by an emergency) as long as they are remembered.
    addresses_by_ttl: PriorityQueue<IpAddr, Instant>,

    /// last time we spawned an async resolution task to resolve this name
    last_resolution_timepoint: Option<Instant>,

    /// is an async resolution task currently outstanding?
    pending_resolution: bool,

    /// reference to the resolver's configuration as we need its fields internally
    config: Arc<SraResolverConfig>,
}

impl HostEntry {
    /// Create a new HostEntry instance
    fn new(config: Arc<SraResolverConfig>) -> HostEntry {
        HostEntry {
            pending_requests: VecDeque::new(),
            ipv4_addresses: GenerationalAddressSet::new(),
            ipv6_addresses: GenerationalAddressSet::new(),
            addresses_by_ttl: PriorityQueue::new(),
            last_resolution_timepoint: None,
            pending_resolution: false,
            config,
        }
    }

    /// Removes an address completely from the host entry.
    fn purge_address(&mut self, address: IpAddr) {
        self.addresses_by_ttl.remove(&address);

        match &address {
            IpAddr::V4(v4addr) => {
                self.ipv4_addresses.remove_address(v4addr);
            }
            IpAddr::V6(v6addr) => {
                self.ipv6_addresses.remove_address(v6addr);
            }
        }
    }

    fn should_cache_address(&self, address: &IpAddr) -> bool {
        match &address {
            IpAddr::V4(_) => self.config.address_types != NameResolutionAddressTypes::IpV6,
            IpAddr::V6(_) => self.config.address_types != NameResolutionAddressTypes::IpV4,
        }
    }

    /// Updates the entry with a newly-resolved address
    fn on_address_resolved(&mut self, address: IpAddr, ttl: Duration) {
        // filter if configured
        if !self.should_cache_address(&address) {
            return;
        }

        let expiration_time = (self.config.clock)() + ttl;

        // If we already know about this address, just extend its TTL, regardless of any other
        // (healthy/unhealthy/banned) status
        if self.addresses_by_ttl.contains(&address) {
            self.addresses_by_ttl
                .change_priority(&address, expiration_time);
            return;
        }

        // track the address by lifetime
        self.addresses_by_ttl.push(address, expiration_time);

        // add it to the correct set
        match address {
            IpAddr::V4(v4addr) => {
                self.ipv4_addresses.add_address(v4addr);
            }
            IpAddr::V6(v6addr) => {
                self.ipv6_addresses.add_address(v6addr);
            }
        }
    }

    /// Mark an address as unhealthy.  If the address is already unhealthy, extend the duration
    /// it will stay in that state.
    fn demote_address(&mut self, address: IpAddr, duration: Duration) {
        let now = (self.config.clock)();
        match address {
            IpAddr::V4(v4addr) => {
                self.ipv4_addresses.demote_address(v4addr, duration, now);
            }
            IpAddr::V6(v6addr) => {
                self.ipv6_addresses.demote_address(v6addr, duration, now);
            }
        }
    }

    /// Removes all addresses whose TTL have expired from the host entry
    fn purge_expired(&mut self) {
        let now = (self.config.clock)();

        while !self.addresses_by_ttl.is_empty() {
            if self.addresses_by_ttl.peek().unwrap().1 <= &now {
                let purged = self.addresses_by_ttl.pop().unwrap().0;
                self.purge_address(purged);
            } else {
                break;
            }
        }
    }

    /// Penalize an address based on the connection error category
    fn handle_address_penalty(&mut self, address: IpAddr, penalty: &HostEventAddressPenalty) {
        match penalty.category() {
            AddressPenaltyCategory::Fatal => {
                // remove from the sets, keep in TTL, effectively banishes the address until
                // forgotten
                match address {
                    IpAddr::V4(v4addr) => {
                        self.ipv4_addresses.remove_address(&v4addr);
                    }
                    IpAddr::V6(v6addr) => {
                        self.ipv6_addresses.remove_address(&v6addr);
                    }
                }
            }
            AddressPenaltyCategory::Major => {
                self.demote_address(address, self.config.resolution_ttl);
            }
            AddressPenaltyCategory::Custom(duration) => {
                self.demote_address(address, *duration);
            }
            _ => {}
        }
    }

    /// Pulls addresses from the cache to complete a resolution request.  If no addresses exist,
    /// fail the request
    fn complete_request(&mut self, request: ResolveRequestInternal) {
        let mut addresses: Vec<IpAddr> = Vec::new();

        let now = (self.config.clock)();
        let v4_addr_option = self.ipv4_addresses.acquire_address(now);
        if let Some(v4_addr) = v4_addr_option {
            addresses.push(IpAddr::V4(v4_addr));
        }

        let v6_addr_option = self.ipv6_addresses.acquire_address(now);
        if let Some(v6_addr) = v6_addr_option {
            addresses.push(IpAddr::V6(v6_addr));
        }

        if addresses.is_empty() {
            (request.completion_callback())(Err(ResolveDnsError::new(
                "No addresses could be resolved",
            )));
        } else {
            (request.completion_callback())(Ok(addresses));
        }
    }

    /// handles a resolution request by appropriately using cached addresses and potentially kicking
    /// off a background resolution
    fn handle_resolve_request(
        &mut self,
        request: ResolveRequestInternal,
        context: &OperationHandlerContext,
    ) {
        let now = (self.config.clock)();
        // if it's our first request or enough time has passed since the last one
        let mut should_spawn_resolve: bool = self.last_resolution_timepoint.is_none();
        if let Some(last_resolution_timepoint) = self.last_resolution_timepoint {
            let refresh_delay = request
                .request()
                .refresh_delay()
                .unwrap_or(self.config.refresh_delay());
            if now >= last_resolution_timepoint + refresh_delay && !self.pending_resolution {
                should_spawn_resolve = true;
            }
        }

        if should_spawn_resolve {
            self.pending_resolution = true;
            self.last_resolution_timepoint = Some(now);

            (context.resolution_function())(request.request().clone());
        }

        // if no addresses are available and we have a background resolution in progress, enqueue
        // the request to be finished when the pending resolution finishes
        if self.ipv4_addresses.empty() && self.ipv6_addresses.empty() && self.pending_resolution {
            self.pending_requests.push_back(request);
            return;
        }

        // complete the request with what we have
        self.complete_request(request);
    }

    /// applies the result of a background resolution task to the host entry.  If resolution was
    /// a failure, then fail all pending requests.  Otherwise, apply all resolved addresses
    /// to the cached state appropriately, then attempt to complete pending requests normally.
    fn handle_resolve_result(&mut self, result: ResolveResult) {
        self.pending_resolution = false;

        match result.result() {
            Ok(addresses) => {
                // lazily purge expired addresses only after a successful resolve.
                self.purge_expired();
                let ttl = result.request().ttl().unwrap_or(self.config.resolution_ttl);
                for address in addresses {
                    self.on_address_resolved(*address, ttl);
                }
            }
            Err(err) => {
                while !self.pending_requests.is_empty() {
                    let pending_request = self.pending_requests.pop_front().unwrap();
                    // we can't actually clone errors but we need to complete one or more requests
                    // with the resolve error.  This is the best I could come up with =/
                    let cloned = ResolveDnsError::new(err.to_string());
                    (pending_request.completion_callback())(Err(cloned));
                }
                return;
            }
        }

        // complete any pending requests with what we have available, if anything
        while !self.pending_requests.is_empty() {
            let pending_request = self.pending_requests.pop_front().unwrap();
            self.complete_request(pending_request);
        }
    }

    /// Applies a host event to a host's entry in the resolver
    fn handle_host_event(&mut self, event: HostEvent) {
        if let HostEventInformation::AddressPenalty(penalty) = event.event_data() {
            self.handle_address_penalty(event.address(), penalty)
        }
    }
}

struct SraCachingResolverState {
    hosts: HashMap<String, HostEntry>,

    sra_config: Arc<SraResolverConfig>,
}

impl SraCachingResolverState {
    fn new(sra_config: SraResolverConfig) -> Self {
        SraCachingResolverState {
            hosts: HashMap::new(),
            sra_config: Arc::new(sra_config),
        }
    }
}

fn handle_resolve_request(
    context: &OperationHandlerContext,
    state: &mut SraCachingResolverState,
    request: ResolveRequestInternal,
) {
    if !state.hosts.contains_key(request.request().name()) {
        let name = request.request().name();
        state
            .hosts
            .insert(name.to_string(), HostEntry::new(state.sra_config.clone()));
    }

    let host_entry = state.hosts.get_mut(request.request().name()).unwrap();

    host_entry.handle_resolve_request(request, context);
}

fn handle_resolve_result(state: &mut SraCachingResolverState, result: ResolveResult) {
    if let Some(entry) = state.hosts.get_mut(result.request().name()) {
        entry.handle_resolve_result(result);
    }
}

fn handle_host_event(state: &mut SraCachingResolverState, event: HostEvent) {
    if let Some(entry) = state.hosts.get_mut(event.name()) {
        entry.handle_host_event(event);
    }
}

fn handle_operation_sra(
    context: &OperationHandlerContext,
    state: &mut SraCachingResolverState,
    operation: ResolverOperation,
) {
    match operation {
        ResolverOperation::ResolveRequest(request) => {
            handle_resolve_request(context, state, request);
        }
        ResolverOperation::ResolveResult(result) => {
            handle_resolve_result(state, result);
        }
        ResolverOperation::HostEvent(event) => handle_host_event(state, event),
    }
}

/// Creates a new caching resolver that follows the SRA specification for domain name resolution
pub fn new_sra_caching_resolver(
    sra_config: SraResolverConfig,
    caching_config: ResolverConfig,
) -> SharedDnsResolver {
    let state = SraCachingResolverState::new(sra_config);
    SharedDnsResolver::new(ResolverImpl::new(
        caching_config,
        state,
        handle_operation_sra,
    ))
}

#[cfg(test)]
mod testing {
    use super::*;
    use aws_smithy_runtime_api::client::dns::{
        AddressPenaltyCategory, HostEventAddressPenalty, HostEventInformation,
    };
    use std::sync::Mutex;

    #[test]
    fn generational_address_set_newly_created_is_empty() {
        let address_set: GenerationalAddressSet<Ipv4Addr> = GenerationalAddressSet::new();

        assert!(address_set.empty());
        assert!(!address_set.contains(&Ipv4Addr::new(127, 0, 0, 1)));
    }

    #[test]
    fn generational_address_set_add_multiple_are_present() {
        let mut address_set: GenerationalAddressSet<Ipv4Addr> = GenerationalAddressSet::new();

        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        address_set.add_address(loopback);
        assert!(!address_set.empty());
        assert!(address_set.contains(&loopback));
        assert_eq!(Some(0), address_set.get_healthy_data(loopback));
        assert_eq!(None, address_set.get_unhealthy_timestamp(loopback));

        let local = Ipv4Addr::new(192, 168, 0, 1);
        address_set.add_address(local);
        assert!(!address_set.empty());
        assert!(address_set.contains(&local));
        assert_eq!(Some(0), address_set.get_healthy_data(local));
        assert_eq!(None, address_set.get_unhealthy_timestamp(local));
    }

    #[test]
    fn generational_address_set_remove_multiple_no_longer_present() {
        let mut address_set: GenerationalAddressSet<Ipv4Addr> = GenerationalAddressSet::new();

        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        address_set.add_address(loopback);
        let local = Ipv4Addr::new(192, 168, 0, 1);
        address_set.add_address(local);

        // local is gone
        address_set.remove_address(&local);
        assert!(!address_set.empty());
        assert!(!address_set.contains(&local));
        assert_eq!(None, address_set.get_healthy_data(local));
        assert_eq!(None, address_set.get_unhealthy_timestamp(local));

        // nothing has happened to loopback yet
        assert!(address_set.contains(&loopback));
        assert_eq!(Some(0), address_set.get_healthy_data(loopback));
        assert_eq!(None, address_set.get_unhealthy_timestamp(loopback));

        // loopback is gone
        address_set.remove_address(&loopback);
        assert!(address_set.empty());
        assert!(!address_set.contains(&loopback));
        assert_eq!(None, address_set.get_healthy_data(loopback));
        assert_eq!(None, address_set.get_unhealthy_timestamp(loopback));
    }

    #[test]
    fn generational_address_set_acquire_multiple_single_generation() {
        let mut address_set: GenerationalAddressSet<Ipv4Addr> = GenerationalAddressSet::new();

        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        address_set.add_address(loopback);
        let local = Ipv4Addr::new(192, 168, 0, 1);
        address_set.add_address(local);

        let now = Instant::now();
        let acquired_addresses: Vec<Ipv4Addr> = vec![
            address_set.acquire_address(now).unwrap(),
            address_set.acquire_address(now).unwrap(),
        ];

        assert!(acquired_addresses.contains(&loopback));
        assert!(acquired_addresses.contains(&local));
        assert_eq!(1, address_set.current_generation());
    }

    #[test]
    fn generational_address_set_acquire_multiple_only_one_available() {
        let mut address_set: GenerationalAddressSet<Ipv4Addr> = GenerationalAddressSet::new();

        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        address_set.add_address(loopback);

        let now = Instant::now();
        let acquired_addresses: Vec<Ipv4Addr> = vec![
            address_set.acquire_address(now).unwrap(),
            address_set.acquire_address(now).unwrap(),
            address_set.acquire_address(now).unwrap(),
        ];

        for address in &acquired_addresses {
            assert_eq!(loopback, *address);
        }

        assert_eq!(3, address_set.current_generation());
    }

    #[test]
    fn generational_address_set_acquire_multiple_multiple_generations() {
        let mut address_set: GenerationalAddressSet<Ipv4Addr> = GenerationalAddressSet::new();

        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        address_set.add_address(loopback);
        let local = Ipv4Addr::new(192, 168, 0, 1);
        address_set.add_address(local);

        let now = Instant::now();
        let acquired_addresses: Vec<Ipv4Addr> = vec![
            address_set.acquire_address(now).unwrap(),
            address_set.acquire_address(now).unwrap(),
            address_set.acquire_address(now).unwrap(),
            address_set.acquire_address(now).unwrap(),
            address_set.acquire_address(now).unwrap(),
            address_set.acquire_address(now).unwrap(),
        ];

        for idx in 0..3 {
            let start_index = idx * 2;
            let end_index = start_index + 2;

            let generation_slice = &acquired_addresses[start_index..end_index];
            assert!(generation_slice.contains(&loopback));
            assert!(generation_slice.contains(&local));
        }

        assert_eq!(3, address_set.current_generation());
    }

    #[test]
    fn generational_address_set_empty_acquire_failure() {
        let mut address_set: GenerationalAddressSet<Ipv4Addr> = GenerationalAddressSet::new();

        let now = Instant::now();

        assert!(address_set.acquire_address(now).is_none());
        assert!(address_set.acquire_address(now).is_none());
    }

    #[test]
    fn generational_address_set_demote() {
        let mut address_set: GenerationalAddressSet<Ipv4Addr> = GenerationalAddressSet::new();

        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        address_set.add_address(loopback);

        let now = Instant::now();
        let unhealthy_duration = Duration::from_secs(10);
        address_set.demote_address(loopback, unhealthy_duration, now);

        assert!(!address_set.empty());
        assert!(address_set.contains(&loopback));
        assert_eq!(None, address_set.get_healthy_data(loopback));
        assert_eq!(
            Some(now + unhealthy_duration),
            address_set.get_unhealthy_timestamp(loopback)
        );
    }

    #[test]
    fn generational_address_set_acquire_unhealthy_promoted_by_time() {
        let mut address_set: GenerationalAddressSet<Ipv4Addr> = GenerationalAddressSet::new();

        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        address_set.add_address(loopback);

        let local = Ipv4Addr::new(192, 168, 0, 1);
        address_set.add_address(local);

        let mut now = Instant::now();
        let unhealthy_duration = Duration::from_secs(10);
        address_set.demote_address(local, unhealthy_duration, now);

        let mut acquired_addresses: Vec<Ipv4Addr> = Vec::new();
        // only loopback should resolve since local is unhealthy for 10 seconds
        acquired_addresses.push(address_set.acquire_address(now).unwrap());
        acquired_addresses.push(address_set.acquire_address(now).unwrap());
        assert_eq!(loopback, acquired_addresses[0]);
        assert_eq!(loopback, acquired_addresses[1]);

        // acquire with advanced "time" so that local should now be healthy
        acquired_addresses.clear();
        now += unhealthy_duration;
        acquired_addresses.push(address_set.acquire_address(now).unwrap());
        acquired_addresses.push(address_set.acquire_address(now).unwrap());
        assert!(acquired_addresses.contains(&loopback));
        assert!(acquired_addresses.contains(&local));
    }

    #[test]
    fn generational_address_set_acquire_unhealthy_promoted_by_necessity() {
        let mut address_set: GenerationalAddressSet<Ipv4Addr> = GenerationalAddressSet::new();

        let loopback = Ipv4Addr::new(127, 0, 0, 1);
        address_set.add_address(loopback);

        let local = Ipv4Addr::new(192, 168, 0, 1);
        address_set.add_address(local);

        let now = Instant::now();
        let unhealthy_duration = Duration::from_secs(10);
        address_set.demote_address(local, unhealthy_duration, now);

        let mut acquired_addresses: Vec<Ipv4Addr> = Vec::new();
        // only loopback should resolve since local is unhealthy for 10 seconds
        acquired_addresses.push(address_set.acquire_address(now).unwrap());
        acquired_addresses.push(address_set.acquire_address(now).unwrap());
        assert_eq!(loopback, acquired_addresses[0]);
        assert_eq!(loopback, acquired_addresses[1]);

        // remove loopback, leaving only an unhealthy local
        address_set.remove_address(&loopback);

        // acquire with current time, forcing local to be promoted
        acquired_addresses.clear();
        acquired_addresses.push(address_set.acquire_address(now).unwrap());
        acquired_addresses.push(address_set.acquire_address(now).unwrap());
        assert_eq!(local, acquired_addresses[0]);
        assert_eq!(local, acquired_addresses[1]);
    }

    #[derive(Clone)]
    struct MockClock {
        now: Arc<Mutex<Instant>>,
    }

    impl MockClock {
        fn new() -> Self {
            Self {
                now: Arc::new(Mutex::new(Instant::now())),
            }
        }

        fn advance_time(&mut self, duration: Duration) {
            let mut current_time = self.now.lock().unwrap();
            *current_time += duration;
        }

        fn get_current_time(&self) -> Instant {
            *self.now.lock().unwrap()
        }

        fn create_clock_wrapper(&self) -> ClockFn {
            let self_clone = self.clone();
            Arc::new(move || self_clone.get_current_time())
        }
    }

    struct HostEntryMockResolveState {
        results: Vec<ResolveDnsResult>,
        current_index: usize,
        should_loop: bool,
        resolutions: VecDeque<ResolveResult>,
        call_count: usize,
    }

    impl HostEntryMockResolveState {
        fn new() -> Self {
            Self {
                results: Vec::new(),
                current_index: 0,
                should_loop: true,
                resolutions: VecDeque::new(),
                call_count: 0,
            }
        }

        fn set_should_loop(&mut self, should_loop: bool) {
            self.should_loop = should_loop;
        }

        fn add_result(&mut self, result: ResolveDnsResult) {
            self.results.push(result);
        }

        fn resolve(&mut self, request: ResolveRequest) {
            self.call_count += 1;

            if self.current_index >= self.results.len() && self.should_loop {
                self.current_index = 0;
            }

            if self.current_index >= self.results.len() {
                let result =
                    ResolveResult::builder(request, Err(ResolveDnsError::new("Out of results")))
                        .build();
                self.resolutions.push_back(result);
                return;
            }

            match &self.results[self.current_index] {
                Ok(addresses) => {
                    let result = ResolveResult::builder(request, Ok(addresses.clone())).build();
                    self.resolutions.push_back(result);
                }
                Err(error) => {
                    let result = ResolveResult::builder(
                        request,
                        Err(ResolveDnsError::new(error.to_string())),
                    )
                    .build();
                    self.resolutions.push_back(result);
                }
            }

            self.current_index += 1;
        }

        fn pop_resolution(&mut self) -> Option<ResolveResult> {
            self.resolutions.pop_front()
        }

        fn get_call_count(&self) -> usize {
            self.call_count
        }
    }

    #[derive(Clone)]
    struct MockResolveFunctionWrapper {
        results: Arc<Mutex<HostEntryMockResolveState>>,
    }

    impl MockResolveFunctionWrapper {
        fn new() -> Self {
            MockResolveFunctionWrapper {
                results: Arc::new(Mutex::new(HostEntryMockResolveState::new())),
            }
        }

        fn set_should_loop(&self, should_loop: bool) {
            self.results.lock().unwrap().set_should_loop(should_loop);
        }

        fn add_result(&self, result: ResolveDnsResult) {
            self.results.lock().unwrap().add_result(result);
        }

        fn resolve(&self, request: ResolveRequest) {
            self.results.lock().unwrap().resolve(request)
        }

        fn pop_resolution(&self) -> Option<ResolveResult> {
            self.results.lock().unwrap().pop_resolution()
        }

        fn get_call_count(&self) -> usize {
            self.results.lock().unwrap().get_call_count()
        }
    }

    struct HostEntryTestContext {
        entry: HostEntry,
        config: Arc<SraResolverConfig>,
        operation_handler_context: OperationHandlerContext,
        mock_clock: MockClock,
        mock_results: MockResolveFunctionWrapper,
    }

    impl HostEntryTestContext {
        fn new(mut config_builder: SraResolverConfigBuilder) -> Self {
            let mock_clock = MockClock::new();
            config_builder.with_clock(mock_clock.create_clock_wrapper());

            let mock_results = MockResolveFunctionWrapper::new();
            let results_clone = mock_results.clone();
            let mock_resolution_function: ResolutionFunction =
                Box::new(move |request| results_clone.resolve(request));

            let config = Arc::new(config_builder.build());

            Self {
                entry: HostEntry::new(config.clone()),
                config,
                operation_handler_context: OperationHandlerContext {
                    resolution_function: mock_resolution_function,
                },
                mock_clock,
                mock_results,
            }
        }

        fn advance_time(&mut self, duration: Duration) {
            self.mock_clock.advance_time(duration);
        }
    }

    impl Default for HostEntryTestContext {
        fn default() -> Self {
            HostEntryTestContext::new(SraResolverConfig::builder())
        }
    }

    const MOCK_HOST: &str = "hello.there.com";
    const LOCALHOST_IPV4: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    const LOCALHOST_IPV6: IpAddr = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));
    const OTHERHOST_IPV4: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1));
    const OTHERHOST_IPV6: IpAddr = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 192, 168, 0, 1));

    fn initialize_resolution_test_context_single(context: &mut HostEntryTestContext) {
        context.mock_results.set_should_loop(true);
        context
            .mock_results
            .add_result(Ok(vec![LOCALHOST_IPV4, LOCALHOST_IPV6]));
    }

    type MockResultTarget = Arc<Mutex<Option<ResolveDnsResult>>>;

    fn create_mock_internal_request(
        ttl_override: Option<Duration>,
        refresh_delay_override: Option<Duration>,
    ) -> (ResolveRequestInternal, MockResultTarget) {
        let result = Arc::new(Mutex::new(None));
        let result_clone = result.clone();
        let mut request_builder = ResolveRequest::builder(MOCK_HOST);
        if let Some(duration) = ttl_override {
            request_builder.with_ttl(duration);
        }

        if let Some(duration) = refresh_delay_override {
            request_builder.with_refresh_delay(duration);
        }

        let internal_request = ResolveRequestInternal {
            request: request_builder.build(),
            completion_callback: Box::new(move |res| {
                result_clone.lock().unwrap().replace(res);
            }),
        };

        (internal_request, result)
    }

    fn is_result_localhost(result_target: MockResultTarget) -> bool {
        let final_result_option = result_target.lock().unwrap().take();
        assert!(final_result_option.is_some());

        let final_result = final_result_option.unwrap();
        assert!(final_result.is_ok());

        let addresses = final_result.unwrap();
        addresses.contains(&LOCALHOST_IPV6) && addresses.contains(&LOCALHOST_IPV6)
    }

    fn apply_mock_result_to_context(context: &mut HostEntryTestContext) {
        // our resolve function just stuffs a result here, pull it out
        let pending_result = context.mock_results.pop_resolution();
        assert!(pending_result.is_some());

        // apply the resolve result to the host entry and verify expected state
        context.entry.handle_resolve_result(pending_result.unwrap());

        assert!(context.entry.pending_requests.is_empty());
        assert!(!context.entry.pending_resolution);
    }

    fn do_host_entry_basic_lookup_and_resolve_test(
        context: &mut HostEntryTestContext,
        ttl_override: Option<Duration>,
    ) {
        let (request, result_target) = create_mock_internal_request(ttl_override, None);

        // submit a request and then check expected internal state
        context
            .entry
            .handle_resolve_request(request, &context.operation_handler_context);

        // expected internal state:
        //   the request is pending
        //   no result has been applied/completed to the request
        //   a resolution is ongoing (running in the background)
        //   last resolution timepoint is now
        assert!(!context.entry.pending_requests.is_empty());
        assert!(result_target.lock().unwrap().take().is_none());
        assert!(context.entry.pending_resolution);
        let last_resolution_timepoint = context.entry.last_resolution_timepoint;
        assert!(last_resolution_timepoint.is_some());
        assert_eq!(
            context.mock_clock.get_current_time(),
            last_resolution_timepoint.unwrap()
        );

        // advance time a little
        context.advance_time(Duration::from_millis(10));

        // apply result and verify requests not pending and resolution not pending
        apply_mock_result_to_context(context);

        // additional expected:
        //   theres now an ipv4 entry in the cache
        //   theres now an ipv6 entry in the cache
        //   both ipv4 and ipv6 addresses have the correct ttl (now + ttl)
        assert!(!context.entry.ipv4_addresses.empty());
        assert!(!context.entry.ipv6_addresses.empty());
        let ttl = ttl_override.unwrap_or(context.config.resolution_ttl);
        let expected_expiration = context.mock_clock.get_current_time() + ttl;
        assert_eq!(
            expected_expiration,
            *context
                .entry
                .addresses_by_ttl
                .get(&LOCALHOST_IPV4)
                .unwrap()
                .1
        );
        assert_eq!(
            expected_expiration,
            *context
                .entry
                .addresses_by_ttl
                .get(&LOCALHOST_IPV6)
                .unwrap()
                .1
        );

        // what did our request actually get resolved with?  we expect localhost for both ipv4 and
        // ipv6
        assert!(is_result_localhost(result_target));

        // background resolution function should only have been called once
        assert_eq!(1, context.mock_results.get_call_count());
    }

    #[test]
    fn host_entry_basic_lookup_and_resolve() {
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_single(&mut context);

        do_host_entry_basic_lookup_and_resolve_test(&mut context, None);
    }

    #[test]
    fn host_entry_basic_lookup_and_resolve_with_ttl_override() {
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_single(&mut context);

        do_host_entry_basic_lookup_and_resolve_test(&mut context, Some(Duration::from_secs(10)));
    }

    fn submit_requests(
        context: &mut HostEntryTestContext,
        count: usize,
        delay_millis: u64,
    ) -> Vec<MockResultTarget> {
        let mut result_targets = Vec::new();

        for _ in 0..count {
            let (request, result_target) = create_mock_internal_request(None, None);
            context
                .entry
                .handle_resolve_request(request, &context.operation_handler_context);

            result_targets.push(result_target);

            if delay_millis > 0 {
                context.advance_time(Duration::from_millis(delay_millis));
            }
        }

        result_targets
    }

    #[test]
    fn host_entry_multiple_requests_before_resolve_finishes() {
        // similar to previous test, but submit multiple resolve requests before handling the result
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_single(&mut context);

        let result_targets = submit_requests(&mut context, 5, 100);
        assert_eq!(5, context.entry.pending_requests.len());

        // apply result and verify requests not pending and resolution not pending
        apply_mock_result_to_context(&mut context);

        // what did our requests actually get resolved with?  we expect localhost for both ipv4 and
        // ipv6
        for result_target in result_targets {
            assert!(is_result_localhost(result_target));
        }

        // background resolution function should only have been called once
        assert_eq!(1, context.mock_results.get_call_count());
    }

    fn initialize_resolution_test_context_multiple(context: &mut HostEntryTestContext) {
        context.mock_results.set_should_loop(true);
        context
            .mock_results
            .add_result(Ok(vec![LOCALHOST_IPV4, LOCALHOST_IPV6]));
        context
            .mock_results
            .add_result(Ok(vec![OTHERHOST_IPV4, OTHERHOST_IPV6]));
    }
    fn do_results_contain_both_host_sets(result_targets: Vec<MockResultTarget>) -> bool {
        let mut contains_local = false;
        let mut contains_other = false;

        result_targets
            .into_iter()
            .map(|target| target.lock().unwrap().take().unwrap().unwrap())
            .for_each(|addresses| {
                if addresses.contains(&LOCALHOST_IPV4) && addresses.contains(&LOCALHOST_IPV6) {
                    contains_local = true;
                } else if addresses.contains(&OTHERHOST_IPV4) && addresses.contains(&OTHERHOST_IPV6)
                {
                    contains_other = true;
                }
            });

        contains_local && contains_other
    }

    fn submit_requests_with_refresh_delay_override(
        context: &mut HostEntryTestContext,
        count: usize,
        delay_millis: u64,
        refresh_delay: Duration,
    ) -> Vec<MockResultTarget> {
        let mut result_targets = Vec::new();

        for _ in 0..count {
            let (request, result_target) = create_mock_internal_request(None, Some(refresh_delay));
            context
                .entry
                .handle_resolve_request(request, &context.operation_handler_context);

            result_targets.push(result_target);

            if delay_millis > 0 {
                context.advance_time(Duration::from_millis(delay_millis));
            }
        }

        result_targets
    }

    fn do_host_entry_multiple_lookups_multiple_results_test(
        context: &mut HostEntryTestContext,
        refresh_delay_override: Option<Duration>,
    ) {
        // in this test, we have the mock resolution alternate between two address sets, rather than
        // one
        //
        // We verify that only the first set is seen until we let enough time elapse that a
        // second background resolution attempt is done and then we should see the resolver
        // use both sets
        let result_targets_pre = submit_requests(context, 2, 100);
        assert_eq!(2, context.entry.pending_requests.len());

        // apply result and verify requests not pending and resolution not pending
        apply_mock_result_to_context(context);

        for result_target in result_targets_pre {
            assert!(is_result_localhost(result_target));
        }

        // let enough time elapse that we re-resolve next request
        let expected_delay = refresh_delay_override.unwrap_or(DEFAULT_REFRESH_DELAY);
        context.advance_time(expected_delay);

        let result_targets = match refresh_delay_override {
            Some(duration) => submit_requests_with_refresh_delay_override(context, 1, 0, duration),
            None => submit_requests(context, 1, 0),
        };

        // should have used the cached results, but also a resolve should be pending because enough
        // time has passed
        for result_target in result_targets {
            assert!(is_result_localhost(result_target));
        }
        assert!(context.entry.pending_requests.is_empty());
        assert!(context.entry.pending_resolution);

        // apply the pending resolve
        apply_mock_result_to_context(context);

        // make a couple requests, we should get both address sets now immediately
        let result_targets_post = submit_requests(context, 3, 100);
        assert!(do_results_contain_both_host_sets(result_targets_post));
    }

    #[test]
    fn host_entry_multiple_lookups_multiple_results() {
        // in this test, we have the mock resolution alternate between two address sets, rather than
        // one
        //
        // We verify that only the first set is seen until we let enough time elapse that a
        // second background resolution attempt is done and then we should see the resolver
        // use both sets
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_multiple(&mut context);

        do_host_entry_multiple_lookups_multiple_results_test(&mut context, None);
    }

    #[test]
    fn host_entry_multiple_lookups_multiple_results_with_refresh_delay_override() {
        // in this test, we have the mock resolution alternate between two address sets, rather than
        // one
        //
        // We verify that only the first set is seen until we let enough time elapse that a
        // second background resolution attempt is done and then we should see the resolver
        // use both sets
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_multiple(&mut context);

        do_host_entry_multiple_lookups_multiple_results_test(
            &mut context,
            Some(Duration::from_secs(1)),
        );
    }

    fn do_address_restricted_lookup_test(address_types: NameResolutionAddressTypes) {
        let mut restricted_builder = SraResolverConfig::builder();
        restricted_builder.with_address_types(address_types);
        let mut context = HostEntryTestContext::new(restricted_builder);
        initialize_resolution_test_context_single(&mut context);

        let result_targets = submit_requests(&mut context, 3, 100);
        assert_eq!(3, context.entry.pending_requests.len());

        // apply result and verify requests not pending and resolution not pending
        apply_mock_result_to_context(&mut context);

        // all results should just be a single address of the appropriate type
        for result_target in result_targets {
            let addresses = result_target.lock().unwrap().take().unwrap().unwrap();
            assert_eq!(1, addresses.len());

            if address_types == NameResolutionAddressTypes::IpV4 {
                assert_eq!(LOCALHOST_IPV4, addresses[0]);
            } else if address_types == NameResolutionAddressTypes::IpV6 {
                assert_eq!(LOCALHOST_IPV6, addresses[0]);
            }
        }
    }

    #[test]
    fn host_entry_lookup_ipv4_only() {
        do_address_restricted_lookup_test(NameResolutionAddressTypes::IpV4);
    }

    #[test]
    fn host_entry_lookup_ipv6_only() {
        do_address_restricted_lookup_test(NameResolutionAddressTypes::IpV6);
    }

    #[test]
    fn host_entry_addresses_expire_manual() {
        // resolve and then let enough time pass to expire the original addresses
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_multiple(&mut context);

        let result_targets = submit_requests(&mut context, 1, 100);
        assert_eq!(1, context.entry.pending_requests.len());

        // apply result
        apply_mock_result_to_context(&mut context);

        // check ttl
        let expected_ttl = context.mock_clock.get_current_time() + context.config.resolution_ttl;
        let addresses = result_targets[0].lock().unwrap().take().unwrap().unwrap();
        for address in addresses {
            assert_eq!(
                expected_ttl,
                *context.entry.addresses_by_ttl.get(&address).unwrap().1
            );
        }

        // let enough time pass, manually purge expired, and then check address state
        context
            .mock_clock
            .advance_time(context.config.resolution_ttl);
        context.entry.purge_expired();
        assert!(context.entry.addresses_by_ttl.is_empty());
        assert!(context.entry.ipv6_addresses.empty());
        assert!(context.entry.ipv4_addresses.empty());
    }

    #[test]
    fn host_entry_addresses_expire_lazy() {
        // resolve and then let enough time pass to expire the original addresses
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_multiple(&mut context);

        let _ = submit_requests(&mut context, 1, 100);
        assert_eq!(1, context.entry.pending_requests.len());

        // apply result
        apply_mock_result_to_context(&mut context);

        let current_addresses: Vec<IpAddr> = context
            .entry
            .addresses_by_ttl
            .iter()
            .map(|val| val.0.clone())
            .collect();
        assert!(!current_addresses.is_empty());

        // let enough time pass, manually purge expired, and then check address state
        context
            .mock_clock
            .advance_time(context.config.resolution_ttl);

        // new resolve with result
        let _ = submit_requests(&mut context, 1, 100);

        // should still have old addresses
        for address in &current_addresses {
            assert!(context.entry.addresses_by_ttl.contains(address));
        }

        // apply result, which should trigger a purge of the old, expired addresses
        apply_mock_result_to_context(&mut context);

        // previous address set should be gone/replaced
        for address in &current_addresses {
            assert!(!context.entry.addresses_by_ttl.contains(address));
        }

        // background resolution function should have been called twice
        assert_eq!(2, context.mock_results.get_call_count());
    }

    fn do_host_entry_fatal_host_event_bans_address(address_to_ban: IpAddr) {
        let matching_address = if address_to_ban == LOCALHOST_IPV4 {
            LOCALHOST_IPV6
        } else {
            LOCALHOST_IPV4
        };

        let expected_other_address = if address_to_ban == LOCALHOST_IPV4 {
            OTHERHOST_IPV4
        } else {
            OTHERHOST_IPV6
        };

        // set up resolution with multiple address sets
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_multiple(&mut context);

        // make a resolve request
        let result_targets = submit_requests(&mut context, 1, 100);
        assert_eq!(1, context.entry.pending_requests.len());
        apply_mock_result_to_context(&mut context);

        assert!(is_result_localhost(result_targets[0].clone()));

        // mark supplied address as a fatal error, should never be seen again as long as ttl active
        let address_penalty =
            HostEventAddressPenalty::builder(AddressPenaltyCategory::Fatal).build();
        let address_penalty_event: HostEvent = HostEvent::builder(
            MOCK_HOST,
            address_to_ban,
            HostEventInformation::AddressPenalty(address_penalty),
        )
        .build();
        context.entry.handle_host_event(address_penalty_event);
        // still know about it
        assert!(context.entry.addresses_by_ttl.contains(&address_to_ban));
        // but it's not in the specific list
        match address_to_ban {
            IpAddr::V4(_) => {
                assert!(context.entry.ipv4_addresses.empty());
            }
            IpAddr::V6(_) => {
                assert!(context.entry.ipv6_addresses.empty());
            }
        }

        // make a resolve request, should not see banned address in the cached reply
        let result_targets = submit_requests(&mut context, 1, 100);
        let addresses = result_targets[0].lock().unwrap().take().unwrap().unwrap();
        assert_eq!(1, addresses.len());
        assert!(addresses.contains(&matching_address));

        // advance time enough to get a second background resolution
        context.mock_clock.advance_time(DEFAULT_REFRESH_DELAY);

        // make enough resolve requests to be guaranteed to see both address sets, verify
        // address_to_ban never shows up
        let _ = submit_requests(&mut context, 1, 100);
        apply_mock_result_to_context(&mut context);

        let result_targets = submit_requests(&mut context, 5, 100);
        for result_target in result_targets {
            let addresses = result_target.lock().unwrap().take().unwrap().unwrap();
            // every response should contain otherhost for the appropriate address type
            assert!(addresses.contains(&expected_other_address));
            // banned address not seen
            assert!(!addresses.contains(&address_to_ban));
        }

        // background resolution function should have been called two times
        assert_eq!(2, context.mock_results.get_call_count());
    }

    #[test]
    fn host_entry_fatal_host_event_bans_ipv4_address() {
        do_host_entry_fatal_host_event_bans_address(LOCALHOST_IPV4);
    }

    #[test]
    fn host_entry_fatal_host_event_bans_ipv6_address() {
        do_host_entry_fatal_host_event_bans_address(LOCALHOST_IPV6);
    }

    #[test]
    fn host_entry_reresolving_a_banned_ip_does_not_cause_it_to_be_used() {
        // set up resolution with single address set
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_single(&mut context);

        // make a resolve request
        let result_targets = submit_requests(&mut context, 1, 100);
        assert_eq!(1, context.entry.pending_requests.len());
        apply_mock_result_to_context(&mut context);

        assert!(is_result_localhost(result_targets[0].clone()));

        // mark LOCALHOST_IPV4 as a fatal error, should never be seen again as long as ttl active
        let address_penalty =
            HostEventAddressPenalty::builder(AddressPenaltyCategory::Fatal).build();
        let address_penalty_event: HostEvent = HostEvent::builder(
            MOCK_HOST,
            LOCALHOST_IPV4,
            HostEventInformation::AddressPenalty(address_penalty),
        )
        .build();
        context.entry.handle_host_event(address_penalty_event);
        // still know about it
        assert!(context.entry.addresses_by_ttl.contains(&LOCALHOST_IPV4));
        // but it's not in the specific list
        assert!(context.entry.ipv4_addresses.empty());

        // advance time enough to get a second background resolution
        context.mock_clock.advance_time(DEFAULT_REFRESH_DELAY);

        // make enough resolve requests to trigger a background resolution which will contain
        // LOCALHOST_IPV4, then make some more requests and verify LOCALHOST_IPV4 never shows up
        let _ = submit_requests(&mut context, 1, 100);
        apply_mock_result_to_context(&mut context);

        let result_targets = submit_requests(&mut context, 5, 100);
        for result_target in result_targets {
            let addresses = result_target.lock().unwrap().take().unwrap().unwrap();
            assert_eq!(1, addresses.len());
            assert!(addresses.contains(&LOCALHOST_IPV6));
        }

        // background resolution function should have been called two times
        assert_eq!(2, context.mock_results.get_call_count());
    }

    #[test]
    fn host_entry_bans_expire() {
        // set up resolution with single address set
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_single(&mut context);

        // make a resolve request
        let result_targets = submit_requests(&mut context, 1, 100);
        assert_eq!(1, context.entry.pending_requests.len());
        apply_mock_result_to_context(&mut context);

        assert!(is_result_localhost(result_targets[0].clone()));

        // mark LOCALHOST_IPV4 as a fatal error, should never be seen again as long as ttl active
        let address_penalty =
            HostEventAddressPenalty::builder(AddressPenaltyCategory::Fatal).build();
        let address_penalty_event: HostEvent = HostEvent::builder(
            MOCK_HOST,
            LOCALHOST_IPV4,
            HostEventInformation::AddressPenalty(address_penalty),
        )
        .build();
        context.entry.handle_host_event(address_penalty_event);
        // still know about it
        assert!(context.entry.addresses_by_ttl.contains(&LOCALHOST_IPV4));
        // but it's not in the specific list
        assert!(context.entry.ipv4_addresses.empty());

        // advance time enough to forget about the ban; because purging addresses is lazy,
        // there's no way to verify forgetting about it unless we call an internal api
        // (purge_expired) or re-resolve which will re-add it
        context
            .mock_clock
            .advance_time(context.config.resolution_ttl);

        // make a resolve requests to trigger a background resolution which will contain
        // LOCALHOST_IPV4, then make some more requests and verify LOCALHOST_IPV4 now shows up again
        let _ = submit_requests(&mut context, 1, 100);
        apply_mock_result_to_context(&mut context);

        let result_targets = submit_requests(&mut context, 5, 100);
        for result_target in result_targets {
            assert!(is_result_localhost(result_target));
        }

        // background resolution function should have been called two times
        assert_eq!(2, context.mock_results.get_call_count());
    }

    #[test]
    fn host_entry_unhealthy_address_does_not_get_used_if_alternative_exists() {
        // set up resolution with multiple address sets
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_multiple(&mut context);

        // make a resolve request and apply it
        let _ = submit_requests(&mut context, 1, 100);
        assert_eq!(1, context.entry.pending_requests.len());
        apply_mock_result_to_context(&mut context);

        // wait enough time for another background resolution to be triggerable
        context.mock_clock.advance_time(DEFAULT_REFRESH_DELAY);

        // make a resolve request and apply it
        let _ = submit_requests(&mut context, 1, 100);
        apply_mock_result_to_context(&mut context);

        // mark LOCALHOST_IPV6 as unhealthy for a while
        let penalty_duration = Duration::from_secs(5);
        let address_penalty =
            HostEventAddressPenalty::builder(AddressPenaltyCategory::Custom(penalty_duration))
                .build();
        let address_penalty_event: HostEvent = HostEvent::builder(
            MOCK_HOST,
            LOCALHOST_IPV6,
            HostEventInformation::AddressPenalty(address_penalty),
        )
        .build();
        context.entry.handle_host_event(address_penalty_event);
        // still know about it
        assert!(context.entry.addresses_by_ttl.contains(&LOCALHOST_IPV6));

        // tbis is awkward
        let v6addr: Ipv6Addr = LOCALHOST_IPV6.to_string().parse().unwrap();

        // it's not healthy
        assert!(!context
            .entry
            .ipv6_addresses
            .healthy_addresses
            .contains(&v6addr));
        // it is unhealthy and its expiration time is expected
        assert!(context
            .entry
            .ipv6_addresses
            .unhealthy_addresses_by_expiration
            .contains(&v6addr));
        let expected_expiration = context.mock_clock.get_current_time() + penalty_duration;
        assert_eq!(
            expected_expiration,
            context
                .entry
                .ipv6_addresses
                .unhealthy_addresses_by_expiration
                .get(&v6addr)
                .unwrap()
                .1
                 .0
        );

        // do a bunch of resolves which should just cycle through the cache, but LOCALHOST_IPV6 should
        // be skipped
        let result_targets = submit_requests(&mut context, 5, 0);
        for result_target in result_targets {
            let addresses = result_target.lock().unwrap().take().unwrap().unwrap();
            // localv6 was banned so every resolve should contain otherhostv6
            assert_eq!(2, addresses.len());
            assert!(addresses.contains(&OTHERHOST_IPV6));
            assert!(addresses.contains(&LOCALHOST_IPV4) || addresses.contains(&OTHERHOST_IPV4));
        }

        // background resolution function should have been called two times
        assert_eq!(2, context.mock_results.get_call_count());
    }

    #[test]
    fn host_entry_unhealthy_address_gets_used_if_no_alternative() {
        // similar to the base unhealthy test, but we only use a single address set which will force
        // a resurrection of the unhealthy address

        // set up resolution with a single address set
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_single(&mut context);

        // make a resolve request and apply it
        let _ = submit_requests(&mut context, 1, 100);
        assert_eq!(1, context.entry.pending_requests.len());
        apply_mock_result_to_context(&mut context);

        // mark LOCALHOST_IPV6 as unhealthy for a while
        let address_penalty =
            HostEventAddressPenalty::builder(AddressPenaltyCategory::Major).build();
        let address_penalty_event: HostEvent = HostEvent::builder(
            MOCK_HOST,
            LOCALHOST_IPV6,
            HostEventInformation::AddressPenalty(address_penalty),
        )
        .build();
        context.entry.handle_host_event(address_penalty_event);

        let v6addr: Ipv6Addr = LOCALHOST_IPV6.to_string().parse().unwrap();
        assert!(!context
            .entry
            .ipv6_addresses
            .healthy_addresses
            .contains(&v6addr));
        assert!(context
            .entry
            .ipv6_addresses
            .unhealthy_addresses_by_expiration
            .contains(&v6addr));

        // do a resolve which should resurrect the address because there's no alternative
        let result_targets = submit_requests(&mut context, 1, 0);

        // it's healthy again
        assert!(context
            .entry
            .ipv6_addresses
            .healthy_addresses
            .contains(&v6addr));
        assert!(!context
            .entry
            .ipv6_addresses
            .unhealthy_addresses_by_expiration
            .contains(&v6addr));

        for result_target in result_targets {
            let addresses = result_target.lock().unwrap().take().unwrap().unwrap();
            assert_eq!(2, addresses.len());
            assert!(addresses.contains(&LOCALHOST_IPV6));
            assert!(addresses.contains(&LOCALHOST_IPV4));
        }
    }

    #[test]
    fn host_entry_unhealthy_recovers_if_enough_time_passes() {
        // similar to the base unhealthy test, but after showing it's not being used,
        // we let enough time pass for the address to become healthy again
        // set up resolution with multiple address sets
        let mut context = HostEntryTestContext::default();
        initialize_resolution_test_context_multiple(&mut context);

        // make a resolve request and apply it
        let _ = submit_requests(&mut context, 1, 100);
        assert_eq!(1, context.entry.pending_requests.len());
        apply_mock_result_to_context(&mut context);

        // wait enough time for another background resolution to be triggerable
        context.mock_clock.advance_time(DEFAULT_REFRESH_DELAY);

        // make a resolve request and apply it
        let _ = submit_requests(&mut context, 1, 100);
        apply_mock_result_to_context(&mut context);

        // mark LOCALHOST_IPV6 as unhealthy for a while
        let penalty_duration = Duration::from_secs(5);
        let address_penalty =
            HostEventAddressPenalty::builder(AddressPenaltyCategory::Custom(penalty_duration))
                .build();
        let address_penalty_event: HostEvent = HostEvent::builder(
            MOCK_HOST,
            LOCALHOST_IPV6,
            HostEventInformation::AddressPenalty(address_penalty),
        )
        .build();
        context.entry.handle_host_event(address_penalty_event);
        // still know about it
        assert!(context.entry.addresses_by_ttl.contains(&LOCALHOST_IPV6));

        let v6addr: Ipv6Addr = LOCALHOST_IPV6.to_string().parse().unwrap();
        // it's not healthy
        assert!(!context
            .entry
            .ipv6_addresses
            .healthy_addresses
            .contains(&v6addr));
        // it is unhealthy and its expiration time is expected
        assert!(context
            .entry
            .ipv6_addresses
            .unhealthy_addresses_by_expiration
            .contains(&v6addr));
        let expected_expiration = context.mock_clock.get_current_time() + penalty_duration;
        assert_eq!(
            expected_expiration,
            context
                .entry
                .ipv6_addresses
                .unhealthy_addresses_by_expiration
                .get(&v6addr)
                .unwrap()
                .1
                 .0
        );

        // advance time enough for the unhealthy categorization to fall off
        context.mock_clock.advance_time(penalty_duration);

        // do a bunch of resolves which should just cycle through the cache; we should see all
        // addresses
        let result_targets = submit_requests(&mut context, 3, 0);
        assert!(do_results_contain_both_host_sets(result_targets));
    }

    #[test]
    fn host_entry_resolve_failure() {
        // make the mock background resolution function return errors, verify resolves fail
        let mut context = HostEntryTestContext::default();
        // by not initializing any results, resolution will fail

        // make some resolve requests and apply the background resolve failure
        let result_targets = submit_requests(&mut context, 3, 100);
        assert_eq!(3, context.entry.pending_requests.len());
        apply_mock_result_to_context(&mut context);

        for result_target in result_targets {
            let res = result_target.lock().unwrap().take().unwrap();
            assert!(res.is_err());
        }

        // make some more resolve requests, they should fail immediately due to an empty cache
        // and not enough time has passed to re-resolve
        let result_targets = submit_requests(&mut context, 3, 100);
        assert_eq!(0, context.entry.pending_requests.len());
        assert!(!context.entry.pending_resolution);

        for result_target in result_targets {
            let res = result_target.lock().unwrap().take().unwrap();
            assert!(res.is_err());
        }

        // advance time enough for a re-resolve (which will also fail)
        context.mock_clock.advance_time(DEFAULT_REFRESH_DELAY);

        // make some more resolve requests; they'll trigger a re-resolve but also ultimately
        // fail when the background resolution fails
        let result_targets = submit_requests(&mut context, 3, 100);
        assert_eq!(3, context.entry.pending_requests.len());
        assert!(context.entry.pending_resolution);

        apply_mock_result_to_context(&mut context);

        for result_target in result_targets {
            let res = result_target.lock().unwrap().take().unwrap();
            assert!(res.is_err());
        }

        // background resolution function should have been called two times
        assert_eq!(2, context.mock_results.get_call_count());
    }
}
