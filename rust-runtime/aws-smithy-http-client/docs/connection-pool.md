# HTTP Connection Pool

> **Draft.** Terminology and Appendix A are written; the remaining sections are outlined only. Every type
> named in later prose must already appear in Terminology or Appendix A. A section wanting a type absent
> from both is a signal to reconcile the surface rather than introduce a name.

## Requirements

### Preserve existing HTTP client behavior

The pool becomes the client every generated smithy-rs client uses by default. A caller who never asked
for pooling still gets it, so any observable protocol difference is a breaking change for code that did
not opt in.

The client supports HTTP and HTTPS, HTTP/1.1 and HTTP/2, direct and proxied connections, DNS overrides,
connect and read timeouts, connection poisoning, and connection metadata capture. Pooling does not alter
request-target form, proxy authentication, TLS negotiation, timeout scope, response-body ownership, or
error classification.

The default client enables HTTP/1.1 and HTTP/2 and lets connector ALPN select the protocol. Client-wide
HTTP/2-only configuration remains distinct from connector ALPN configuration. A request marked HTTP/2 does
not dispatch on an HTTP/1.1 connection; a request marked HTTP/1.1 may dispatch on HTTP/2. Request version
neither selects an ALPN policy nor forms part of the pool key.

### Bound connections to one origin

Callers sharing an origin with other tenants, or running against a service with per-client connection
limits, need a ceiling they can state. `max_connections_per_host = N` bounds admitted connections to one
host across every partition. Connecting, handshaking, open active, and open idle connections all count
against the bound. Another host uses an independent bound. The default is unbounded, and a configured
value of zero is rejected rather than producing a client that cannot make progress.

The bound counts connections the pool has admitted, not sockets the operating system currently holds. No
request dispatches on a connection that has begun closing.

### Conserve capacity

Capacity is a linear quantity: a fixed number of permits exist, and each is held by exactly one owner.
Every path that can end early — cancellation at checkout, admission, connect, handshake, publication,
dispatch, or body read; runtime shutdown; panic; loss of the underlying I/O — returns or transfers what it
owns. A cancelled waiter retains no connection, no generation, and no capacity.

An asynchronous component's futures can be dropped at any await point, so this cannot be left to the paths
someone remembered to handle. Capacity that no owner accounts for is capacity the pool can never issue
again, and a pool that loses permits degrades monotonically toward refusing all work.

Runtime shutdown drops both pool-spawned protocol futures and futures the HTTP implementation spawned on
the caller's behalf. A request holds a strong reference to its pool through receipt of response headers or
a terminal error, so dropping the last external reference cannot race a request in flight.

### Keep I/O on its owning runtime

A connection's socket and protocol tasks stay on the partition that created them for the connection's
lifetime. Reuse from another partition moves no I/O, no interface binding, no accounting, and no idle
residence.

Callers place partitions deliberately — a runtime per core, a runtime per NUMA node, a partition per
network interface. Migrating a connection's I/O would silently undo that placement: bytes would leave from
an interface the caller did not choose, or be driven by a runtime the caller pinned elsewhere. This holds
under every reuse scope, including the most permissive.

### Callers own the topology

The caller declares the pool's shape and the pool does not infer one. A partition names the runtime its
connection drivers run on and, optionally, the network interface its sockets bind to. The caller chooses
how many partitions exist, what identifies them, and how broadly connections may be reused across them.

The pool cannot derive this. Whether two runtimes should share connections depends on why they are
separate — thread-per-core placement, tenant isolation, interface separation — and that reason exists only
in the caller's design. Reuse breadth follows from the same authority: a caller wanting strict locality and
a caller wanting maximum reuse are both right about their own workload.

Absent configuration a caller gets one anonymous partition, which is the shape of a program that has not
thought about placement. Partitions with no interface configured are the common case and compare as one
group without per-request interface work.

### No request starves

A request that can neither reuse nor create a connection parks without polling. Returning a reusable
connection, publishing an HTTP/2 generation, or releasing capacity makes an eligible parked request
runnable.

Ordering need not be exactly FIFO, but scheduling is work-conserving among eligible waiters, and later
arrivals do not bypass a committed waiter indefinitely. Admission contention introduces no unbounded
latency tail as partition or waiter count grows: a pool that admits work at an acceptable median while
some requests wait tens of seconds has failed this requirement, not merely underperformed.

Conserved capacity is not sufficient. Capacity can be intact and still unreachable — held by a connection
no eligible waiter may use. Capacity embodied by an out-of-scope HTTP/1 connection does not remain
stranded while that connection repeatedly returns reusable. The pool does not proactively drain active
HTTP/2 connections for this purpose, so progress requires an eligible reusable connection, a reclaimable
HTTP/1 transition, or released capacity; it is not promised while every permit is held indefinitely by
active out-of-scope HTTP/2 work.

### Cost scales with use, not configuration

A caller pays for the features they configure. An unconfigured pool constructs no coordination machinery
and its hot path touches none: no admission state, no cross-partition structures, no host-wide
coordination. Configuration a caller does not use costs nothing at runtime, not merely little.

Cost also does not grow with topology. Local reuse performs bounded work independent of partition count
and does not serialize through pool-wide state. Coordination that spans partitions runs only after local
reuse misses.

Both halves are about the same failure. Machinery that exists for a configured feature, but sits on the
path every request takes, taxes callers who never enabled it — and a shared structure on the hot path
costs most exactly where the pool is meant to help, under many partitions issuing small requests
concurrently.

### Make pool behavior and state observable

Operators diagnose connection problems — unexpected establishment rates, connections closing earlier than
expected, reuse not happening — and cannot do so from request outcomes alone. The pool reports connection
lifecycle events and per-partition statistics, each identifying the physical connection's owning partition
and negotiated protocol.

Reporting does not compromise the pool. Listeners run outside pool locks and are synchronous: a listener
may delay the request or task that invokes it, and must not block or wait on pool work. A panicking
listener neither skips lifecycle cleanup nor reorders events.

## Architecture

*Not yet written.*

## Terminology

Terms whose everyday meaning would otherwise mislead. Everything else is defined where it is first used.

**Partition** — a driver-spawner runtime and an optional network-interface binding, identified by a
caller-owned `PartitionId`.

**Host** — a `(scheme, authority)` pair. One `HostPool` per host.

**Shard** — the partition × host join, `PartitionHostPool`. Its identity is stable until pool drop.

**Source** and **target** — roles in the borrow protocol: the source shard holds a reusable HTTP/1
handle, the target shard wants one.

**Permit** — the conserved unit of connection capacity. Linear: created, moved, destroyed, never copied.
*Capacity* is the aggregate quantity permits account for, used in sums and bounds.

**Demand** — a shard's standing signal that it could use one more connection. One fixed ticket per shard,
not one per request, so demand cannot accumulate.

**Borrow** — moving a dispatch handle to a peer shard, leaving the connection open. Transfers no I/O
authority.

**Reclaim** — closing a connection so its permit can move to another shard. Transfers capacity, not I/O.

**Lease** and **handle** — a lease is a hold on capacity and owns a permit; a handle is a hold on a
connection and does not.

**Publish** and **deliver** — publication makes state visible to many unnamed readers; delivery hands one
value to one waiting party.

**Attempt** and **flight** — an HTTP/1 establishment is an attempt, independent of other attempts; an
HTTP/2 establishment is a flight, coordinated so at most one is in progress per shard.

**Logical close** — a connection stops accepting new work and releases its permit. **Physical close** —
the socket is gone. Physical close follows logical close by an unbounded interval.

**Generation** — an HTTP/2 connection's dispatch epoch, a first-class object with a lifecycle.

**Revision** — a monotonic version on a published snapshot; readers retain the newest and discard older.

**Episode** — a bounded activity admitting at most one terminal outcome. Work naming a superseded episode
is rejected.

## Correctness invariants

*Not yet written.* Fixed three-part shape: the invariant, what it rules out, the mechanism enforcing it.
Each invariant states its own enforcing mechanism here; Appendix B enumerates the tests exercising them
and carries no part of the argument.

## Open questions

**Undeclared-partition lookup.** `Client::from_partition` panics when the pool has no such partition,
treating it as a construction-time programming error. A fallible constructor would surface it as a value
instead. Resolved by whether any caller constructs partitions dynamically enough that the identifier can be
wrong at runtime; every caller so far derives it from a fixed thread numbering established before the pool
is built.

**Obtaining an `Authority`.** `ConnectionPool::stats` takes `&Authority`, and nothing else in the public surface
produces one, so a caller must know how to construct it. Whether `stats` should accept something a caller
already holds — a `&str` or a `&Uri` — depends on what the lookup costs: an `Authority` that must be parsed
or allocated per call makes a polling caller pay for it repeatedly, which also bears on how cheap the
observation surface is to sample.

## Future work

*Not yet written.*

---

## Appendix A: Public API and module structure

### Construction

A pool is built once and shared; clients are cheap handles onto it.

```rust
pub struct ConnectionPool;

impl ConnectionPool {
    pub fn builder() -> Builder<TlsUnset>;
    pub fn stats(&self, authority: &Authority) -> AuthorityStats;
}

pub struct Client;

impl Client {
    pub fn new(pool: &ConnectionPool) -> Self;
    pub fn from_partition(pool: &ConnectionPool, id: PartitionId) -> Self;
}
```

`Client` implements the smithy runtime's `HttpClient`. This is the pool's purpose: it is the replacement
for the default HTTP client every generated smithy-rs client uses, so a pool-backed client is configured
wherever any other client would be.

`Client::new` uses the anonymous partition; `from_partition` names a declared one.

`stats` returns sparse per-partition snapshots keyed by `PartitionId` — only partitions that have observed
the authority appear.

### Builder

TLS provider selection is the only typestate transition, and it gates only TLS configuration. Every other
setting is available in either state. Each setter has a `set_*` mirror taking `&mut self` and an `Option`,
for callers assembling configuration programmatically.

```rust
pub struct Builder<Tls = TlsUnset>;

impl<Tls> Builder<Tls> {
    pub fn idle_timeout(self, timeout: impl Into<Option<Duration>>) -> Self;
    pub fn tcp_nodelay(self, nodelay: bool) -> Self;
    pub fn tcp_keepalive(self, time: impl Into<Option<Duration>>) -> Self;
    pub fn max_connections_per_host(self, n: usize) -> Self;
    pub fn connection_reuse_scope(self, scope: ConnectionReuseScope) -> Self;
    pub fn proxy_config(self, config: ProxyConfig) -> Self;
    pub fn dns_resolver(self, resolver: impl ResolveDns + 'static) -> Self;
    pub fn connection_event_listener(self, listener: impl ConnectionEventListener + 'static) -> Self;
    pub fn partitions(self, partitions: impl IntoIterator<Item = Partition>) -> Self;
}

impl Builder<TlsUnset> {
    pub fn tls_provider(self, provider: tls::Provider) -> Builder<TlsProviderSelected>;

    pub fn build_http(self) -> ConnectionPool;
    pub fn build_http_with_tcp_connector<C, IO>(self, connector: C) -> ConnectionPool;
}

impl Builder<TlsProviderSelected> {
    pub fn tls_context(self, context: TlsContext) -> Self;
    pub fn build_https(self) -> ConnectionPool;
}
```

`max_connections_per_host` is unset by default, and an unset bound constructs no admission machinery. Set,
it bounds connections to one host across all partitions and interface groups — not per partition.

### Partitions

```rust
pub struct PartitionId(u64);

impl PartitionId {
    pub const fn from_index(index: usize) -> Self;
}

pub struct Partition;

impl Partition {
    pub fn new<S: DriverSpawner>(id: PartitionId, spawner: S) -> Self;
    pub fn interface(self, nic: impl Into<String>) -> Self;
}

pub trait DriverSpawner: Debug + Send + Sync + 'static {
    fn spawn(&self, driver: Pin<Box<dyn Future<Output = ()> + Send + 'static>>);
}

pub struct TokioDriverSpawner;

impl TokioDriverSpawner {
    pub fn current() -> Self;
    pub fn from_handle(handle: tokio::runtime::Handle) -> Self;
}
```

A partition names a runtime to spawn connection drivers on and, optionally, a network interface to bind
its sockets to.

`PartitionId` is caller-owned because partition identity almost always derives from a numbering the caller
already maintains — a thread index, a core index, a shard number. A thread-per-core caller assigns
`PartitionId::from_index(thread_id)` and can reconstruct the same identity at every site that needs it:
declaring partitions, constructing per-thread clients, and correlating `AuthorityStats` back to threads or
interfaces. Pool-issued opaque handles would require plumbing a handle to each of those sites, and any
caller keying a map by partition would need the handle to be hashable, which reintroduces an identifier.

`TokioDriverSpawner::current` captures the handle eagerly and panics outside a runtime context;
`from_handle` takes a specific handle, and drivers run on the runtime that handle refers to regardless of
which runtime calls `spawn`.

`Partition::interface` is offered on the platforms where the binding can be applied, so an unsupported
binding cannot be configured silently. The binding is applied to the socket before connect —
`SO_BINDTODEVICE` on Linux-like systems, `IP_BOUND_IF` on macOS-like and Solaris-like ones — which fixes a
connected socket's egress interface for the socket's lifetime.

### Reuse scope

```rust
pub enum ConnectionReuseScope {
    Partition,
    NetworkInterface,  // default
    Pool,
}
```

Scope bounds which shards may borrow a dispatch handle from one another. It does not bound reclaim, which
transfers capacity rather than I/O authority.

### Observation

```rust
pub trait ConnectionEventListener: Send + Sync {
    fn connection_created(&self, event: &ConnectionCreatedEvent);
    fn connection_reused(&self, event: &ConnectionReusedEvent);
    fn connection_borrowed(&self, event: &ConnectionBorrowedEvent);
    fn connection_closed(&self, event: &ConnectionClosedEvent);
    fn connection_failed(&self, event: &ConnectionFailedEvent);
}

pub struct AuthorityStats;
pub struct PartitionStats;

pub enum NegotiatedProtocol { Http1, Http2 }
pub enum CloseReason { /* … */ }
pub struct ConnectionTiming;
pub struct Authority;
```

### Module structure

```
aws-smithy-http-client/src/client/
  pool.rs              — ConnectionPool, PoolKey, HostPool, re-exports
  pool/
    builder.rs         — Builder typestate, connector assembly, interface binding
    client.rs          — Client
    partition.rs       — Partition, PartitionId, DriverSpawner, TokioDriverSpawner
    admission/         — HostAdmission, demand tickets, permits, claim table
    connection.rs      — ConnectionRecord, lifecycle events, listener interface
    handshake.rs       — establishment: HTTP/1 attempts, HTTP/2 flights
    stats.rs           — AuthorityStats, PartitionStats, counters
```

`pool::{builder, client, partition}` are public; the rest is private. Types a caller names are re-exported
from `pool`, so callers write `pool::Client` rather than `pool::client::Client`.

The connector contract is unchanged: a connector is a `Service<Uri>` yielding `(IO, Connected)`. The pool
composes connectors and does not replace the contract.

---

## Appendix B: Validation

*Not yet written.* What is tested, at which level, and what each level rules out. Carries no part of any
correctness argument.
