# HTTP Connection Pool

## Requirements

### Existing HTTP client behavior is preserved

For equivalent configuration, the client preserves the existing client's observable behavior for HTTP
and HTTPS, HTTP/1.1 and HTTP/2, direct and proxied connections, DNS overrides, connect and read
timeouts, connection poisoning, and connection metadata capture.

This includes request-target form, proxy authentication, TLS negotiation, timeout scope, response-body
ownership, and error classification. Any difference requires an explicit compatibility
decision rather than an implicit change in the pool.

### Connections to one origin are bounded

`max_connections_per_host = N` bounds admitted connections to one origin across every partition.
An origin is a scheme, host, and port, so HTTP and HTTPS are bounded separately, as is each
non-default port. Connecting, handshaking, open active, and open idle connections count against the bound.
Each origin has an independent bound. The default is unbounded, and a configured value of zero is rejected.

The bound applies to connections admitted by the pool, not to sockets still held by the operating system
while replaced connections finish tearing down. It is therefore not a file-descriptor ceiling;
[connection retirement](#connection-retirement-and-maintenance) defines that distinction.

### Connection placement follows declared topology

The caller declares the fixed partition set and the maximum scope of connection reuse. Each partition is
a placement scope for establishment and protocol drivers, with an optional network interface. A
connection's transport and protocol tasks remain on the partition that created it for their lifetime;
cross-partition reuse moves dispatch authority, not I/O placement, interface binding, or accounting.

Without explicit partitions, the pool has exactly one anonymous, unbound partition. It binds to one Tokio
runtime on first establishment and may be used across that runtime's worker threads.

### Eligible requests make progress

A request that can neither reuse nor create a connection parks without polling. Under the stated executor
and connection-progress assumptions, scheduling is work-conserving among eligible waiters, later arrivals
do not bypass a committed waiter indefinitely, and each resource grant performs work bounded independently
of waiter and partition count.

The guarantee applies while an eligible reusable connection, a reclaimable HTTP/1 transition, or a
released permit can become reachable. [Liveness](#liveness) defines the progress assumptions and the
cross-scope HTTP/2 condition in which no such resource becomes reachable.

### Local reuse is partition-local and bounded

A local reuse hit performs bounded work independent of partition count. It consults no origin-wide
coordination and reads no other partition's state. An unbounded origin constructs no admission, peer-index,
or cross-partition ordering machinery; cross-partition coordination begins only after local reuse misses on
a bounded origin with no free capacity.

### Pool behavior and state are observable

The pool reports connection lifecycle events and per-partition statistics sufficient to diagnose
establishment, reuse, waiting, logical close, and physical teardown. Events identify the origin and owning
partition, and installed connections also identify the stable connection and negotiated protocol.
Statistics distinguish establishment, admitted H1 and H2 state, draining state, active H2 streams, waiters,
and physically live transports.

## Architecture

The architecture proceeds from topology and ownership through local selection,
establishment, bounded coordination, dispatch, retirement, and telemetry. The
model below summarizes the state and request path; the later sections define
each contract.

### The model

The pool sits between Hyper and a connector. Hyper provides the HTTP/1.1 and HTTP/2 implementations:
[`handshake`](https://github.com/hyperium/hyper/blob/v1.11.0/src/client/conn/http1.rs#L140) yields a
[`SendRequest`](https://github.com/hyperium/hyper/blob/v1.11.0/src/client/conn/http1.rs#L23) dispatch handle
and a connection driver future, and behind those live the request/response state machines, HPACK, and flow
control. Transport arrives as a connector, a `Service<Uri>` yielding `(IO, Connected)`; the pool builds on
that interface without altering it, so the connectors this client already assembles for TLS, proxies, and
tests compose unchanged.

What the pool owns is the layer between them: which connection a request dispatches on, when establishment
starts, how much capacity exists and who holds it, whether a connection may be reused from another runtime,
and when an idle connection closes.

#### Topology

Connections are indexed by two things that vary independently.

A **partition** is a placement scope for connection establishment and drivers, plus an optional network
interface its sockets bind to. An explicit partition names its runtime; the anonymous partition binds to the
current Tokio runtime on first use. The set is fixed at construction.

An **origin** is a scheme, host, and port — the web's origin as
[RFC 6454](https://www.rfc-editor.org/rfc/rfc6454) defines it, canonicalized so two spellings of one server
are one origin. The pool discovers origins lazily from requests rather than declaring them at construction.

A connection belongs to exactly one of each: one partition established it, and it can serve one origin. Their
intersection is an **`OriginCell`**, which holds the connections a partition has for an origin and is created
on first use of that pair.

```text
ConnectionPool
|-- Partition P0 (runtime 0, eth0)
|   |-- OriginCell(P0, s3)
|   `-- OriginCell(P0, dynamodb)
|-- Partition P1 (runtime 1, eth1)
|   `-- OriginCell(P1, s3)
`-- bounded-origin coordination
    |-- OriginAdmission(s3)
    |     `-- P0 and P1 cells
    `-- OriginAdmission(dynamodb)
          `-- P0 cell
```

Because the partition set is fixed and the origin set is not, partitions are the outer level: each partition
owns its own map from origin to cell, so the structure that grows is always inside one partition. A cell that
no request has asked for does not exist.

An **`OriginAdmission`** holds what all partitions sharing a bounded origin must agree on: its connection
budget, cross-cell demand order, and index of cells for that origin. It stores the shared `OriginKey` once and
keys its internal cell, demand, and availability records by `PartitionId`; the origin component is invariant
inside this authority. Nothing else spans partitions.

#### Ownership and lifetime

```text
ConnectionPool
`-- PartitionRegistry
    |-- PartitionState by PartitionId
    |   `-- OriginCell by OriginKey       local waiters and protocol records
    `-- OriginAdmission by OriginKey      bounded-origin permits and ordering

Client
|-- ConnectionPool
`-- resolved PartitionState
```

| Type              | Created                                | Destroyed                                     | Shared across partitions |
| ----------------- | -------------------------------------- | --------------------------------------------- | ------------------------ |
| `ConnectionPool`  | by the builder                         | when the last pool, client, and request release it | —                        |
| `Partition`       | at construction, from the declared set | at pool drop                                  | no                       |
| `OriginCell`      | first request for (partition, origin)  | not while the origin is live                  | no                       |
| `OriginAdmission` | first request for a *bounded* origin   | not while the pool lives                      | yes                      |
| `Client`          | by the caller, freely                  | by the caller                                 | —                        |

`Client` is what a caller holds and what implements the smithy runtime's `HttpClient`. It pairs the pool with
one resolved partition, so a request never searches for its partition — the handle already names it.

An `OriginAdmission` exists only for a bounded origin. A local miss normally
establishes on the requesting partition. When no permit is free, admission may
use compatible peer protocol state or reclaim peer capacity for that demand.
An unbounded origin never needs cross-partition admission or peer indexes; its
cells are independent. Reuse scope controls which peer protocol state is
compatible, while reclaim may recover capacity across eligibility groups.

Pool retention, request accounting, root-I/O ownership, and bounded capacity
have different lifetimes.

```text
pool lifetime

caller-held Client ------------------------------> ConnectionPool
request future, until response head/error -------> ConnectionPool
```

A `Client` and an in-flight request through response headers retain the pool.
Producing a response head or terminal error ends the request future's pool
hold. Protocol-specific guards own the remaining cleanup:

```text
post-header protocol lifetime

H1Exchange <------------------ response body or readiness task
PhysicalConnectionGuard <----- driver or upgraded root I/O

H2 request lease
  |-- receive endpoint <------ response body or upgrade bridge
  `-- send endpoint <--------- accepted H2 request-body adapter
```

An H1 exchange owns Hyper's exclusive HTTP/1 request handle (`SendRequest`) and
returns it only after a reusable message boundary. An
H2 request lease releases only after both stream endpoints terminate. Root I/O
may move from the driver into an upgrade while the same physical guard tracks
pool ownership. Bounded capacity has a separate owner path:

```text
bounded connection capacity

OriginAdmission
  `-- issue --> EstablishmentPermit
                  +-- failure/drop ------------------------> OriginAdmission
                  `-- install --> ConnectionState owns CapacityLease
                                      `-- logical close ---> OriginAdmission
```

A bounded permit moves from admission to establishment and then to the
installed `ConnectionState`. Logical close returns it. Dispatch handles,
`DispatchGuard`, and H2 request leases never own a connection permit.

#### Request path

A request resolves its partition-local cell and first attempts compatible
local selection. A miss registers one acquisition. Capacity and available
protocol state determine how that acquisition completes.

```text
Client(partition P, request URI)
`-- resolve OriginCell(P, origin)
    |
    |-- compatible local connection ------------------> dispatch
    |
    `-- local miss -> register one acquisition
        |
        |-- unbounded origin or free permit
        |   `-- establish on P owner runtime ----------> dispatch
        |
        `-- bounded origin at capacity
            |-- compatible peer connection ------------> dispatch
            |-- reclaimable peer capacity
            |   `-- close peer; transfer permit
            |       `-- establish on P owner runtime --> dispatch
            `-- otherwise park until state changes

selected protocol handle
`-- commit against logical close -> Hyper
    |-- request completes ------------> return or release protocol state
    |-- protocol upgrade ------------> caller owns upgraded lifecycle
    `-- connection-terminal failure -> logical then physical close
```

[Local connection selection](#local-connection-selection) defines local selection.
[Connection establishment](#connection-establishment) defines establishment, placement, and protocol
convergence. [Bounded-capacity coordination](#bounded-capacity-coordination) defines parking, borrow, reclaim,
and resource delivery, with [Liveness](#liveness) stating when those paths guarantee progress.
[Dispatching and completing a request](#dispatching-and-completing-a-request) defines request preparation,
stale-reuse retry, and the transfer to response or upgrade ownership. The path ends in
[return or retirement](#connection-retirement-and-maintenance), where each terminal outcome either makes the
connection reusable or closes it.

### Topology and identity

The model introduces the partition-origin shape. This section defines the public identities, placement
contract, stable cells, and canonical origin key that implement it.

#### Partitions

`PartitionId` is a stable caller-chosen identity for an explicit partition. `Partition` packages that identity,
a driver spawner, and an optional network interface as immutable construction-time state. The fields remain
private because callers configure placement through constructors rather than inspecting its representation.

```rust
/// Stable identity used to construct clients and correlate telemetry.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PartitionId(/* private */);

impl PartitionId {
    /// Reserved identity for the implicit default partition.
    pub const ANONYMOUS: Self;
    pub const fn from_index(index: usize) -> Self;
    pub const fn is_anonymous(self) -> bool;
}

/// Construction-time placement for connections and their protocol drivers.
pub struct Partition {
    /* private: identity, driver spawner, and optional network interface */
}

impl Partition {
    pub fn new<S: DriverSpawner>(id: PartitionId, spawner: S) -> Self;
    pub fn interface(self, nic: impl Into<String>) -> Self;
}

/// Spawns protocol drivers on a partition's owning runtime.
pub trait DriverSpawner: Debug + Send + Sync + 'static {
    fn spawn(&self, driver: Pin<Box<dyn Future<Output = ()> + Send + 'static>>);
}

/// A driver spawner backed by a captured Tokio runtime handle.
pub struct TokioDriverSpawner {
    /* private: captured tokio::runtime::Handle */
}

impl TokioDriverSpawner {
    pub fn current() -> Self;
    pub fn from_handle(handle: tokio::runtime::Handle) -> Self;
}
```

For example, a thread-per-core caller can declare one partition from the identity and runtime it already
maintains:

```rust
Partition::new(PartitionId::from_index(core), TokioDriverSpawner::current())
    .interface("eth0")
```

The caller declares partitions and the pool infers none. Whether two runtimes should share connections
depends on why they are separate, and that reason exists only in the caller's design: a thread-per-core
service separates runtimes for cache locality, a multi-tenant host for isolation, a multi-interface host to
drive independent links. The pool can observe that several runtimes exist but not which of these is true, and
the three want different behavior.

Drivers are spawned only through their partition's `DriverSpawner` and never move once spawned. That is what
keeps a connection's I/O on the runtime that established it, whichever partition later dispatches on it: a
reused connection carries only its dispatch handle across the boundary, never its driver.

`Partition::interface` configures placement through the default HTTP connector.
The binding is applied before connect, so a connected socket retains its egress
placement when another partition uses its dispatch authority. Interface
existence, permissions, and other host-specific failures are connector errors
reported during establishment. Custom connector construction has no pool-level
interface-placement contract.

A pool with no declared partitions has exactly one unbound owner partition. Its first use binds that anonymous
partition to one Tokio runtime for connection-owned work; requests may originate on that runtime or on other
runtimes, and establishment, drivers, and pending return work are submitted back to the captured owner.
Partitions without an interface compare as one group, so the common case performs no per-request interface
work. The anonymous partition has the reserved identity `PartitionId::ANONYMOUS`, used by events and
statistics; callers cannot declare it explicitly. Every explicit identifier is caller-owned. A thread-per-core
caller can therefore reconstruct `PartitionId::from_index(thread_id)` when it declares the
topology, creates each thread's client, and reads per-partition statistics, without plumbing pool-issued
handles between those sites.

`TokioDriverSpawner::current` captures the current Tokio handle eagerly and panics when called outside a
runtime. `from_handle` takes a specific handle. Both spawn on the captured runtime regardless of which thread
invokes `spawn`; neither is supplied by the caller for the anonymous partition, which captures its runtime on
first use as [Connection establishment](#connection-establishment) describes.

##### Alternatives

**Pool-issued opaque partition handles.** Partition identity almost always derives from a numbering the caller
already maintains — a thread index, a core index, a worker number — and the caller needs that identity at
several independent sites: declaring partitions, constructing per-thread clients, and correlating statistics
back to threads or interfaces. An opaque handle must be plumbed to every one of those sites, and any caller
keying a map by partition needs it hashable, which reintroduces an identifier with extra steps.

**Deriving partitions from runtime detection.** Requires the pool to answer why runtimes are separate, which
it cannot observe.

#### Origins and cells

`OriginKey` is the owned public identity used by statistics, events, and callers that need to name an origin.
It semantically contains an HTTP or HTTPS scheme, a canonical host, and an optional non-default port. Its
storage representation is private so lookup and retained-key storage can evolve independently.

```rust
/// An owned, canonical HTTP or HTTPS origin.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OriginKey {
    /* private: scheme, canonical host, and optional non-default port */
}

impl OriginKey {
    pub fn from_uri(uri: &Uri) -> Result<Self, InvalidOrigin>;
    pub fn from_parts(
        scheme: Scheme,
        host: impl AsRef<str>,
        port: Option<u16>,
    ) -> Result<Self, InvalidOrigin>;

    pub fn scheme(&self) -> &Scheme;
    pub fn host(&self) -> &str;
    pub fn port(&self) -> Option<u16>;
}

#[derive(Debug)]
pub struct InvalidOrigin { /* private */ }
```

Two connections are interchangeable exactly when they share an origin, which is why this and not something
narrower or wider is the key: a server enforces its per-client connection limit at this granularity, and TLS
parameters are negotiated for it. `https://example.com` and `https://example.com:8443` are distinct origins,
as are the `http` and `https` forms of one authority.

The key is a canonicalized origin, not the raw `(scheme, authority)` a URI carries, because two spellings of
one server must not become two origins — each with its own `OriginAdmission`, together admitting twice the
bound against a host that sees one. Canonicalization elides the scheme's default port, so `https://x` and
`https://x:443` are one key, and drops userinfo, which the origin does not include and TLS does not vary on.
Host comparison is already ASCII-case-insensitive. Two spellings are not unified: an
internationalized host and its punycode form (the connector resolves what it is given; equating them would
pull in Unicode normalization), and a fully-qualified name with a trailing dot, which is a distinct DNS name.
IPv6 literals are parsed and normalized to the standard compressed spelling so equivalent address text has
one identity. A URI zone identifier remains case-sensitive and is retained exactly; it is an opaque
interface-scoped value rather than a DNS name. Unknown IP-literal forms are not case-folded.

The request path does not construct an owned `OriginKey` merely to probe a partition's origin map. It first
builds a private canonical lookup key that borrows the URI host when its bytes already have canonical form and
owns temporary host storage only when normalization requires it. One possible private representation uses
`Cow<'a, str>` for the host; the exact equivalent-key and map machinery is not part of the public contract.

```text
URI -> private canonical lookup key
         +-- host already canonical ----> borrow URI host
         `-- host needs normalization --> temporary owned host
                    |
                    +-- map hit  -> use existing cell; discard lookup key
                    `-- map miss -> convert once to owned OriginKey and insert
```

An already-canonical hit therefore allocates no host storage. A miss converts the lookup key into the map's
owned key once; a non-canonical hit may allocate temporary normalized bytes but does not retain another key.
`OriginKey::from_uri` and `OriginKey::from_parts` instead construct owned public values and perform the same
canonicalization. `from_parts` lets an observer construct a key once without reparsing a URI for every
statistics sample. It validates the host with the structured HTTP authority parser. Both constructors reject a
scheme other than HTTP or HTTPS, an invalid host or port, and any input that does not name an origin.
`InvalidOrigin` carries the offending component and source error for diagnostics, implements `Error`, and
exposes no second, less strict public key representation.

Canonicalization distinguishes an absent port from malformed, zero, or
out-of-range port text. Invalid explicit ports cannot alias the scheme default.

The request's HTTP version is absent. A request marked HTTP/1.1 may dispatch on an HTTP/2
connection, so version is a dispatch-eligibility question decided per connection, not an identity question
decided per origin. Including it would split one origin's connections into two populations that cannot share
capacity, and would make the pool's shape depend on which requests happened to arrive first.

A URI without both a scheme and an authority names no origin and is rejected before any lookup, rather than
being mapped to a placeholder.

#### Stable identity

A cell is not destroyed while its origin is live, and an origin's state is not destroyed while the pool
lives.

Stability is what lets one partition act on another's cell without checking whether it is still there. When a
request cannot find a connection locally it looks to peer cells for the same origin, and every such
cross-partition operation names a cell it did not create. If a cell could disappear between selection and
use, each reference would need a liveness check and a generation to detect a reused slot — and those checks
would sit on the path taken when a request has *already* failed to find a connection, where latency is least
affordable.

Stability is needed only within an origin. A permit belongs to one origin's admission and cannot be spent on
another; a connection to one origin cannot serve a request for a different one. No reference crosses origins,
so an origin and all of its cells form a unit that could in principle be removed together, while removing one
cell from a live origin could not.

The cost is retained memory. Cells accumulate as the origin set grows and are never reclaimed, bounded by
partitions × origins ever touched, so a many-core client reaching many origins retains cells long after their
connections are gone. Stable identity retains those cells; see
[Reclaiming quiescent origins](#reclaiming-quiescent-origins) for reclamation constraints.

##### Alternatives

**Evicting individual cells that hold no connections.** Reclaims memory at the granularity that breaks the
property above: a peer cell selected for a cross-partition operation could be freed before it is used, so
every such reference would carry a generation and a validity check on the slow path. Whole-origin removal
needs neither, because nothing outside an origin references its cells.

#### Obligations

Cells and their identity:

* **Cell stability** [safety] — a cell is not destroyed while its origin is reachable from the pool.
* **Reference validity** [safety] — a reference to a peer cell for a live origin requires no liveness check
  before use.
* **Cell uniqueness** [safety] — at most one cell exists per (partition, origin) pair; concurrent first use
  produces exactly one.
* **Lazy cells** [optimization] — a (partition, origin) pair no request has named has no cell.

Partitions:

* **Driver placement** [safety] — a connection's driver is spawned only through its partition's
  `DriverSpawner`, and never migrates.
* **Binding immutability** [safety] — a partition's interface binding is fixed at construction and applied
  before connect.
* **Default partition** [safety] — a pool with no declared partitions has exactly one anonymous partition;
  its first use binds one owner runtime, every later establishment and driver uses that runtime, and request
  tasks may execute on another runtime.
* **Interface comparison cost** [optimization] — comparing two unbound partitions performs no string work.

Origins:

* **Key totality** [safety] — every dispatched request maps to exactly one origin; a URI lacking scheme or
  authority is rejected before lookup.
* **Canonical key** [safety] — origins equivalent under default-port elision and userinfo removal map to one
  key, so one server is one `OriginAdmission`.
* **Version independence** [safety] — the request's HTTP version does not participate in the origin key.
* **Allocation-free canonical hit** [optimization] — looking up an already-canonical request origin allocates
  no host storage; only normalization or insertion may own host bytes.

### Smithy client boundary

The smithy runtime selects an HTTP connector for an operation by calling
`HttpClient::http_connector(settings, components)`. `Client` returns a `SharedHttpConnector` around a private
`PoolConnector`. This is a cheap request-policy facade over the client's resolved partition, not another pool
or transport stack: constructing one performs no DNS, TLS, Hyper, or admission setup.

```text
Client(pool, partition P): HttpClient
  |
  +-- http_connector(settings A, components A) -> PoolConnector(pool, P, policy A)
  `-- http_connector(settings B, components B) -> PoolConnector(pool, P, policy B)
                                                        |
                                                        `-- one ConnectionPool
                                                            `-- one OriginAdmission per bounded origin
```

The facade clones the complete non-exhaustive `HttpConnectorSettings` and extracts the operation components
its request policy uses. Its `call` moves a `Client` clone and that policy into the request future, which is the
strong pool reference already shown above. A facade may be cached as an implementation optimization, but its
identity is never a partition, origin, pool, or admission key. Differing connect or read timeouts therefore do
not multiply `max_connections_per_host`: every facade for one `Client` reaches the same origin-wide admission
authority.

Connect and read timeouts remain operation policy rather than connection identity. The facade uses the
operation's `AsyncSleep` from `RuntimeComponents` when present, then the same default-sleep fallback as the
existing client. A configured timeout requires the resulting sleep implementation; its absence never disables
the timeout. The read timeout wraps `PoolConnector::call` through response headers. A connect timeout wraps
only the transport-connector operation through TLS and ALPN for an establishment attempt owned by that
request; the Hyper protocol handshake remains under the read timeout. A request joining an existing HTTP/2
flight owns no connector operation, so its connect timeout does not apply to that shared flight; its read
timeout or caller cancellation can remove that participant. If it later becomes an establishment driver, its
own connect timeout applies to that attempt.

Idle maintenance is pool policy, not operation policy. Its `TimeSource` and `AsyncSleep` are fixed by the pool
builder and shared by the partition maintenance tasks; they do not depend on which operation first asks for a
facade. Operation `RuntimeComponents` may vary between smithy clients sharing a pool without changing idle
age or creating another pool.

`Client::validate_base_client_config` runs an idempotent transport preflight so native trust roots, when
applicable, load when this HTTP client is selected rather than on its first request. The preflight opens no
socket and creates no settings-keyed pool or connector cache. `validate_final_config` remains a no-op.
`connector_metadata` reports `hyper/1.x`, preserving the HTTP client identifier used in user-agent metadata.
`Client` and `PoolConnector` use bounded custom `Debug` implementations that report immutable configuration
and the resolved partition, not live origin or cell state.

### Local connection selection

A request arrives on a `Client`, which already names its partition. Reuse is therefore two steps: the
partition's origin map, then the cell.

```text
request on partition P for origin O
  |-- P already resolved by Client
  `-- P.origins[O]                     partition-local origin lookup
      `-- select compatible state      one OriginCell lock
```

That is the entire path for a reuse hit. It performs no origin-wide coordination, reads no other partition's
state, and touches no `OriginAdmission` or peer index. Its synchronization is
the requesting partition's own cell lock. A peer may acquire that lock to
install or settle a bounded cross-cell reuse operation, so the lock can be
contended, but the local hit does not consult origin-wide state. Its work is
independent of partition count, and traffic for another origin does not share
the cell.

A reused connection may be dead: the server can close an idle connection while it sits in the cell, and the
pool learns this only on dispatch. So "take a live idle connection" is provisional until the request is
accepted. Hyper's `try_send_request` returns the unsent request when the connection failed before accepting
it, and that returned request is the retry boundary — a reused connection that fails before acceptance is
transparently retried on a fresh one, invisibly to the caller. A request the connection had already accepted
is not retried here; whether to retry it is the caller's policy, because the pool cannot know the request was
not acted on. This distinguishes a *reused* connection, where a pre-acceptance failure is the expected
stale-idle race and is absorbed, from a *fresh* one, whose failure is a real error the caller sees.

On a local miss, one acquisition attempt may wait for a compatible H1 return
while preparing establishment. The returned H1 and establishment result compete
to complete the launching waiter, and exactly one result commits. If H1 wins
before the connector is first polled, connector work remains lazy and tentative
capacity returns to admission. Once connector polling begins, the pool owns the
establishment attempt through completion even if another H1 serves the
launching waiter. A successful result that loses this race remains available
for later compatible demand; failure releases the attempt's resources without
replacing the result already delivered.

If establishment wins, a concurrent H1 return follows ordinary owning-cell
return handling. If no capacity is available, no establishment attempt starts
and the request waits for bounded capacity or cross-cell reuse. Cancellation
removes the launching waiter but does not cancel a started establishment;
every connection, attempt, lease, and waiter retains one terminal owner.

#### Alternatives

**An origin-keyed map at the top, owning its cells.** Groups a bounded origin's budget with the cells it
governs, which reads well and is how a pool is usually drawn. Its cost is a structure shared by every
partition, growing with the origin count, on the path of every request including local reuse hits: concurrent
small requests across many partitions serialize on it to reach state none of them share. Measured on an
implementation shaped that way, small-object throughput fell by roughly an order of magnitude against the
unpooled client, and the shared structure on the reuse path was the cause.

#### Obligations

* **Local hit locality** [optimization] — a reuse hit performs no origin-wide coordination, reads no other
  partition's state, and does work independent of partition count. Its synchronization is the requesting
  partition's own cell lock, which a peer may also acquire under bounded pressure.
* **Partition-count independence** [optimization] — the work of a reuse hit does not grow with the number of
  partitions.
* **Stale-reuse retry** [safety] — a reused connection that fails before accepting the request is retried on a
  fresh connection; a connection that has already accepted the request is not retried by the pool.
* **Acquisition race ownership** [safety] — a compatible local H1 return and an establishment result commit to
  at most one launching waiter; connector work stays lazy before its first poll, and an attempt that has been
  polled completes under pool ownership independently of that waiter.

### Connection establishment

When a request finds no live connection for its origin on its partition, the cell establishes one. The steps
are the same whether or not a bound is configured: acquire capacity if the origin is bounded, run the
connector to obtain transport, hand the transport to Hyper to get a dispatch handle and a driver, place the
driver, and record the connection in the cell. What follows is where each step runs and who holds the
connection's capacity while it does. The case where a bound is set and no capacity is available is deferred
to [Bounded-capacity coordination](#bounded-capacity-coordination); this section assumes establishment may
proceed.

#### Establishment

A connection's socket, driver, and reactor must all live on one runtime, because Tokio registers a socket with
the I/O reactor of whatever runtime creates it — it captures the current runtime's handle at socket creation
([`poll_evented.rs:111`](https://github.com/tokio-rs/tokio/blob/tokio-1.53.1/tokio/src/io/poll_evented.rs#L111))
— and the connector is what creates the socket. If the driver ran on one runtime while the socket was created
on another, every readiness event would cross runtimes, and the socket's reactor would outlive or predecease
the driver that holds it. So establishment — connector, transport, TLS, ALPN, handshake — and the driver it
produces run on the same runtime.

An **explicit partition** names its owner runtime through `DriverSpawner`. The
**anonymous partition** captures the current Tokio runtime on first use. A
request may be polled on another runtime, so every new connection submits the
still-unpolled connector, transport, TLS/ALPN, and Hyper handshake future to
the partition owner. Completion updates the cell and wakes the requesting task;
the resulting driver and pending return work use the same spawner. This policy
costs one task submission and wake per new connection. Local reuse and dispatch
on an established connection do not pay that handoff.

The submitted establishment future carries its own completion guard. If a spawner discards the future before
polling it, or its owner task is dropped after polling begins, that guard completes the waiter with a terminal
error and drops the still-owned establishment permit or attempt. This is a narrow ownership fallback for the
submitted future, not runtime supervision: `DriverSpawner::spawn` retains Hyper's `spawn -> ()` contract and
does not claim to report runtime health synchronously. Once the first poll claims establishment, normal
attempt completion or this guard is responsible for completing the waiter exactly once.

This transfer keeps socket creation, handshake, and driver polling on one runtime while allowing a
partition-specific `Client` to move between independent requester runtimes. Dispatch may cross that boundary
through Hyper's request handle; connection I/O and the driver never do.

Hyper spawns work of its own, and it follows the connection. An HTTP/2 connection hands Hyper a connection
task at handshake and per-stream and upgrade tasks as it runs, through an
[`Executor`](https://github.com/hyperium/hyper/blob/v1.11.0/src/rt/mod.rs#L45) the caller
supplies; the pool supplies one that forwards to the connection's runtime. HTTP/1 uses the partition spawner
for its connection driver and for readiness work that outlives a response body.

#### Connection ownership

A bounded origin has one permit per admitted connection, and a *lease* is what owns it. Admission issues a
lease to the establishing task; a successful handshake transfers the lease to the connection record, which
holds it until logical close releases it. Requests dispatched on the connection take *handles*, not the lease,
so the many concurrent requests on an HTTP/2 connection share the single permit the record's lease owns. An
unbounded origin has no permit, no lease, and no such chain.

The lease is what makes the chain safe to drop: it is an RAII guard, so a connector error, handshake failure,
cancellation, or runtime shutdown returns the permit to admission before the lease passes to a connection
record. After that transfer, the record remains the sole lease owner and its logical-close transition is the
only path that releases the permit.

Admission stores free capacity as a count. Removing one unit creates a non-`Copy` `Permit` with a
never-reused diagnostic identity. Delivery materializes that value into the `CapacityLease`; lease return
increments the free count rather than storing returned permits. The representation avoids an allocation
on capacity return while the permit and lease types preserve linear ownership.

The spawned connection driver is wrapped by a **driver lifecycle guard** armed only after the record and its
generation-specific close authority exist. The guard holds a non-retaining close handle, not the lease. If
the driver completes, the wrapper requests logical close with `ProtocolClosed` and the driver's source error.
If the wrapper is dropped before completion, including when its owning runtime shuts down, the guard's
`Drop` requests logical close with `OwnerRuntimeShutdown`. Either request races through the record's existing
idempotent close transition, so a prior pool drop, poison, reclaim, or protocol close wins without releasing
capacity twice. Because the handle does not retain the pool, driver tasks cannot form a lifetime cycle with
the records they close. The root-I/O wrapper remains the separate authority for physical completion.

This is capacity conservation on the create and driver paths: the permit has one
owner at every step, and cancellation either drops the establishing lease or logically closes the record
that received it.

#### Readiness

The connector is a Tower `Service<Uri>`, and the pool honors that contract: it drives the connector to ready
before calling it, and issues one call per readiness. The dispatch handle Hyper returns is a different thing.
Its readiness — whether the connection can accept another request — is an inherent
[`SendRequest::poll_ready`](https://github.com/hyperium/hyper/blob/v1.11.0/src/client/conn/http1.rs#L156),
not a Tower `Service`: Hyper has no Tower dependency, and the HTTP/2 form ignores its `Context` entirely
([`http2.rs:97`](https://github.com/hyperium/hyper/blob/v1.11.0/src/client/conn/http2.rs#L97)). Connection
readiness is thus a per-protocol dispatch question the pool answers against the connection's state, and does
not share machinery with the connector's Tower readiness. The two are related only by name.

#### HTTP/1 attempts and HTTP/2 flights

The pool supports HTTP/1.1 and HTTP/2 and lets connector ALPN select the protocol. Request version controls
dispatch compatibility, not connector negotiation: an HTTP/2-marked request cannot dispatch on H1, an
HTTP/1.1-marked request may dispatch on H2, and request version is not part of the origin key.

The two protocols establish differently because they reuse differently. An HTTP/1 connection carries one
request at a time, so a cell that needs more concurrency needs more connections: each establishment is an
independent *attempt*, and several may run at once, each producing a connection for the request that launched
it. An HTTP/2 connection multiplexes, so one connection serves the whole cell; a second connection to an
origin the cell already serves is nearly pure waste. HTTP/2 establishment is therefore a *flight*,
coordinated so the cell keeps at most one accepting HTTP/2 generation — a request arriving during a flight
waits for it rather than starting its own, and the resulting generation is published to compatible waiters.

##### Post-ALPN convergence

The initial API imposes no pool-wide HTTP/1-only or HTTP/2-only policy. Connector ALPN resolves a transport's
protocol only after connect, so concurrent misses may each own a transport until then. Attempts remain
independent when they negotiate H1 and converge on the cell's one flight only when they negotiate H2.

The logical owner carried through this decision is an *establishment authority*: the transport and, on a
bounded origin, its capacity lease. The authority has exactly one owner even though the request waiting for
its result does not own it; cancellation of that request cannot silently drop the transport or permit. After
ALPN, the owner performs one cell-local select-or-join transition before starting the Hyper protocol handshake:

```text
automatic attempt owns transport + optional capacity lease + launching waiter
  |
  +-- ALPN = H1
  |     `-- run H1 handshake
  |           +-- success -> install H1 record; record takes capacity lease
  |           |     +-- launching waiter accepts H1 -> dispatch
  |           |     `-- waiter requires H2 -> retain H1 for compatible demand;
  |           |                              return the existing unsupported-version error
  |           `-- error -> close transport; return lease; fail launching waiter
  |
  `-- ALPN = H2
        `-- atomically inspect this cell's accepting generation and H2 flight
              +-- accepting generation -> register waiter against generation
              |                           close losing transport; return its lease
              +-- flight exists --------> register waiter as follower
              |                           close losing transport; return its lease
              `-- neither exists -------> install flight as driver
                                          flight takes transport + lease
                                            |
                                            +-- H2 handshake succeeds
                                            |     -> install record; record takes lease
                                            |     -> publish generation; serve participants
                                            `-- error
                                                  -> close transport; return lease
                                                  -> fail participants
```

The inspection and either registration or flight installation are one transition under the cell's
coordination. This is the linearization point: many automatic attempts may reach H2 ALPN, but at most one
becomes the flight. A losing authority has not started a Hyper driver, so its guard closes the negotiated
transport and returns its capacity lease; its launching waiter remains represented exactly once, as a
generation user or flight follower. The winning flight installs the connection record before publishing the
generation, so no waiter can observe a generation without a driver and capacity owner behind it. Activation
revalidates the generation identity and accepting state; if either changed after registration, the waiter
returns to acquisition rather than dispatching through stale state.

A waiter that joins a flight or generation retains its original waiter sequence. On publication, the
[generation gate](#http2-publication) orders flight participants and already committed compatible local
waiters together; joining an accepting generation follows that same activation path. Post-ALPN convergence
therefore cannot let a later attempt barge ahead of an older compatible waiter.

Request version changes only compatibility. When an automatic attempt launched by an HTTP/2-marked request
negotiates HTTP/1, the H1 connection remains useful and is handed to compatible local demand or the idle set.
The launching request receives the same unsupported-version classification and connection metadata as the
existing client; it neither dispatches on H1 nor loops establishing until ALPN happens to choose H2.

A flight owns a generation identity, a set of participant waiter identities, and optionally a pool-completion
interest transferred from a started attempt whose launching waiter was served or cancelled. Cancelling one
participant removes only that participant. The flight continues while another participant, compatible demand,
or pool-completion interest remains. The completion interest ends when the attempt installs, converges with
existing state, or fails; it prevents the no-participant rule from cancelling a started acquisition loser. If
none of those interests remains, dropping the establishment authority closes the transport and returns the
lease. A dropped flight task performs the same guarded cleanup and reconciles every still-live participant back
to acquisition, so no waiter remains attached to a missing flight. Completion from an old generation identity
is stale and cannot clear or publish over a successor.

The ownership transfer is therefore fixed at each boundary:

| State               | Transport owner           | Capacity owner                              | Waiting-request owner              |
| ------------------- | ------------------------- | ------------------------------------------- | ---------------------------------- |
| independent attempt | establishment authority   | its optional capacity lease                 | cell waiter entry                  |
| H1 installed        | H1 connection record      | record's capacity lease                     | checked-out H1 guard or cell queue |
| H2 flight driver    | flight authority          | flight's capacity lease                     | flight participant entry           |
| H2 joiner           | existing flight or record | existing owner; own capacity lease returned | participant or activation          |
| H2 published        | H2 connection record      | record's capacity lease                     | H2 request lease after activation  |
| failure or drop     | cleanup guard             | guard until admission return                | terminal result or re-acquisition  |

This is the mechanism behind the miss policy from
[Local connection selection](#local-connection-selection): a partition that misses locally establishes its
own connection. It reaches for a peer's connection only when it cannot establish, which the next section
covers.

#### Obligations

* **Establishment placement** [safety] — the connector, transport setup, TLS, ALPN, and handshake for a
  connection are polled only on its placement runtime: the runtime bound by the anonymous partition's first
  establishment or the runtime named by an explicit partition.
* **Hyper task placement** [safety] — tasks Hyper spawns for a connection run on that connection's partition
  runtime.
* **Single permit owner** [safety] — at every point in establishment a bounded connection's permit has
  exactly one owner, and a failed or dropped establishment returns its permit to admission.
* **Driver termination closes the record** [safety] — normal driver completion and cancellation of the
  guarded driver task both request the record's exactly-once logical close without owning or retaining its
  capacity lease.
* **Connector readiness** [safety] — the connector is driven to ready before each call, and each readiness
  admits one call.
* **Single generation** [safety] — a cell keeps at most one accepting HTTP/2 generation; establishments that
  resolve HTTP/2 collapse to one, and the losing transports close and return their permits.
* **Atomic H2 convergence** [safety] — after automatic ALPN selects H2, one cell-local transition either
  activates an accepting generation, joins the one current flight, or installs the caller as that flight's
  driver before any H2 handshake or publication begins.
* **Losing-attempt cleanup** [safety] — an automatic H2 attempt that joins existing state closes its
  unhandshaken transport and returns its capacity lease while retaining its launching waiter exactly once.
* **Compatibility-preserving H1 result** [safety] — an H1 result is retained for compatible demand; an
  H2-required launching waiter receives the existing unsupported-version result and never dispatches on H1 or
  loops establishment for a different ALPN outcome.
* **Flight cancellation** [safety] — participant cancellation removes only that participant; terminal flight
  drop closes its transport, returns its lease, and leaves no live waiter attached to the retired flight
  identity.

### Bounded-capacity coordination

A bounded origin at its limit cannot establish: admission has no permit to issue. The permit it needs may
still exist — held by a connection this waiter cannot use, or held in another partition's cell — but a
present permit is not a usable one. So two things have to happen. A waiter parks until a permit it can use
becomes available, and capacity that exists elsewhere moves to where the demand is. This section covers how a
cell signals demand, how the pool decides which capacity a waiter may take, the two operations that move
capacity, and how a freed resource is delivered to a parked waiter. All of it is bounded-mode only; an
unbounded origin never reaches here.

#### Demand

A cell's demand is one aggregate state, not a per-request count: a cell either wants another connection or it
does not. The state is active while any request waits and inactive when none do, so it is a single standing
ticket per cell rather than one ticket per waiting request.

The waiting requests queue behind the ticket in arrival order. An arriving permit goes to the request at the
head, which stops waiting; the next becomes the head. The ticket is how returning connections and other
cells find a cell with unmet demand; the queue is how that cell chooses whom to serve first.

The ticket carries one thing beyond its presence: whether the head request can use an HTTP/1 connection. A
peer with a reusable HTTP/1 connection to lend needs to know its offer will be taken before it acts, and a
head that requires HTTP/2 cannot use an HTTP/1 handle. This is the minimum the signal must distinguish;
finer matching is a dispatch-time question, not a demand-signal one.

The logical snapshot published to admission is:

```rust
enum ProtocolRequirement {
    H1Compatible,
    H2Required,
}

enum DemandState {
    Active {
        head: ProtocolRequirement,
        eligibility_group: EligibilityGroup,
    },
    Inactive,
}

struct DemandSnapshot {
    id: DemandId,
    version: SnapshotVersion,
    state: DemandState,
}
```

A `DemandId` names one generation for the cell's current queue head and may receive at most one terminal
acquisition outcome. Its protocol requirement is stable. Serving or cancelling that head retires the
generation; if useful demand remains, the cell creates a successor ID for the new head. `SnapshotVersion` orders
complete replacements for the same ID, so a delayed active publication cannot overwrite retirement. An
inactive snapshot retires the demand. When it must queue, each active demand joins the applicable scheduling
orders at their tails; checked identity allocation does not reuse a demand ID after wraparound.

#### Bounded miss

A bounded miss registers local demand before it asks admission to act. This ordering makes a waiter visible
before a permit, return, or publication can target it. An immediately available permit still follows the same
delivery and acknowledgement path as a later one; there is no separate pre-registration fast path whose races
would need a second proof.

```text
local selection misses on cell C
  |
  +-- register waiter and current DemandSnapshot R under C's lock
  `-- publish complete snapshot R to OriginAdmission
        |
        +-- free permit
        |     `-- create capacity delivery fence for R
        |
        `-- no free permit
              `-- queue R in its origin order and applicable eligibility-group views
                    |
                    +-- compatible local H1/H2 appears
                    |     `-- satisfy locally; retire or replace R
                    |
                    +-- eligible peer H2 appears
                    |     `-- publish generation reference to C
                    |
                    +-- peer H1 is available
                    |     `-- borrow or reclaim for the oldest origin demand
                    |
                    `-- permit is released or reclaimed
                          `-- create capacity delivery fence for R

capacity delivery reaches C
  |
  +-- R is stale, cancelled, or already satisfied -> refunnel permit
  `-- R is live
        +-- final local probe now succeeds -> use local resource; refunnel permit
        `-- still needs a connection ------> move lease to establishing task

borrow delivery reaches C
  |
  +-- R or its reserved waiter is stale -> return H1 to connection-owning cell
  `-- still compatible -----------------> move checked-out H1 guard to waiter

terminal outcome for R
  +-- no useful demand remains -> ticket becomes idle
  `-- useful demand remains ---> publish successor generation at applicable tails
```

The final local probe is part of accepting capacity, under the cell lock. It prevents a permit delivered
concurrently with local return or H2 publication from causing an unnecessary new connection. Local progress,
waiter cancellation, and host delivery therefore race through generation and snapshot-version validation rather than
recall a payload already extracted under another lock.

#### Eligibility and capacity

Two independent questions decide how a waiter is served, and they apply to different actions. *Eligibility*
asks whether this partition may dispatch through a particular connection, which
`ConnectionReuseScope` decides: under the default scope, partitions sharing a network
interface are eligible for each other's connections and partitions on different interfaces are not.
*Capacity* asks whether the origin may open another connection — one budget of `N` for the whole origin,
regardless of how many partitions or interfaces exist.

```rust
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConnectionReuseScope {
    Partition,
    #[default]
    NetworkInterface,
    Pool,
}
```

`Partition` permits no cross-cell dispatch. `NetworkInterface` groups partitions by the exact configured
interface, with all unbound partitions in one group. `Pool` permits reuse across every partition. Scope
controls only dispatch eligibility: it does not constrain reclaim, which closes a connection and transfers
capacity rather than moving I/O authority.

Dispatching through a connection that already exists consumes no capacity; that connection already holds one
of the admitted permits. So an eligible reusable connection serves the waiter whether or not a permit is
free — a pool at its limit still dispatches freely through all `N` of its connections. Capacity is consulted
only when no eligible connection can serve the waiter and the cell must open one. The two questions do not
read each other's state, and only that second case reaches admission: eligibility decides reuse, capacity
decides establishment. Keeping them separate is what lets the two operations below relieve a shortage of one
without disturbing the other.

#### Borrow and reclaim

A bounded miss may use another cell's HTTP/1 connection in two ways.

**Borrow** moves the exclusive Hyper request handle to the requesting cell for
one dispatch. The connection record, driver, socket, runtime, and interface stay
with the connection-owning cell. Borrow is therefore limited to cells in the
same reuse eligibility group.

**Reclaim** logically closes a reusable HTTP/1 connection and returns its
capacity lease to admission. The requesting cell can then establish on its own
partition. Reclaim moves no dispatch or I/O authority and is not limited by the
reuse scope.

Admission retains one origin-wide FIFO and one FIFO per eligibility group over
cells that own an HTTP/1 connection that is idle or may return. A cell appears
at most once in each applicable view. It is removed from both views while a
reuse operation is nonterminal or while a usable local fairness turn is owed,
and is reinserted from its next complete availability report.

Every availability report carries a monotonic revision assigned under the
connection-owning cell lock. Admission ignores an equal or older revision, so
reports crossing the unlocked cell-to-admission boundary cannot hide newer
state. An advertisement is a scheduling hint, not a connection handle or
capacity owner.

Borrow and reclaim share one cross-cell reuse protocol:

```rust
enum ReuseMode {
    Borrow,
    Reclaim,
}

enum ReusePhase {
    Installing,
    Installed,
    Resolving,
    Cancelling,
}

struct ReuseOperation {
    id: ReuseId,
    connection_partition: PartitionId,
    requesting_partition: PartitionId,
    demand: DemandId,
    mode: ReuseMode,
    phase: ReusePhase,
    cancelled: bool,
}

enum H1ReuseReservationState {
    Available,
    Installed(ReuseId),
    Resolving(ReuseId),
}

struct H1ReuseReservation {
    state: H1ReuseReservationState,
    local_turn_owed: bool,
}
```

Admission owns `ReuseOperation`; the connection-owning cell owns
`H1ReuseReservation`. Each connection-owning and requesting cell participates
in at most one nonterminal operation at a time. The operation mode is fixed at
selection. A reclaim selected from the origin-wide order cannot become a borrow
because its cells may belong to different eligibility groups. A borrow could
become a reclaim without violating eligibility, but keeping both modes fixed
avoids adding a second terminal path after installation; a rejected operation
returns to admission for a fresh selection.

An operation installs and resolves without nesting the admission,
connection-owning-cell, or requesting-cell locks:

```text
OriginAdmission owns queued demand R
  |
  `-- select peer connection-owning cell C
        `-- create ReuseOperation K in Installing
              `-- install K under C's lock
        |
        +-- C owes a usable local turn
        |     `-- reject K; R stays queued
        |
        +-- C has an older local H1 candidate
        |     `-- reject K; R stays queued
        |
        +-- idle H1 available
        |     `-- C reservation -> Resolving(K); candidate guard owns H1
        |
        +-- active or returning H1 exists
        |     `-- C reservation -> Installed(K); reserve next reusable return
        |
        `-- no H1 can return
              `-- reject K; R stays queued

Installed(K) + reusable return at C
  `-- C reservation -> Resolving(K); candidate guard owns H1

candidate reaches OriginAdmission
  |
  +-- K or R is stale, cancelled, or already satisfied
  |     `-- candidate guard returns H1 through C's ordinary return path
  |
  +-- K is Borrow
  |     `-- fence R as Delivering(R, D)
  |           `-- commit C's candidate before reserving requesting-cell waiter
  |                 +-- accepted -> waiter owns H1 selection
  |                 `-- rejected -> return H1 to C, then close D
  |
  `-- K is Reclaim
        `-- revalidate under C's lock and attempt logical close
              `-- released permit enters ordinary capacity delivery

terminal C report
  `-- remove K; refresh C's availability; schedule next action
```

A provisional candidate revalidates the connection generation, logical-close
state, idle policy, and matching cell reservation before it becomes an
`H1Selection` or is reclaimed. A failed revalidation returns the request handle
through ordinary owning-cell policy. If another close wins the reclaim race,
the released permit still follows its normal exactly-once admission path.

An installed reservation intercepts a reusable return under the
connection-owning cell lock before the handle can become locally idle. Local
H1-compatible demand that arrives after installation may therefore be
overtaken once. An irreversible borrow or successful reclaim records one local
fairness turn when compatible local demand exists. The next local H1 service
consumes that turn; if compatible demand disappears first, the turn clears.
An H2-required local head cannot consume the turn and does not block a reuse
operation that can make progress.

Cancellation marks an installing or resolving operation stale. An installed
reservation crosses back to the connection-owning cell and is cleared. A
candidate already outside the cell lock returns through ordinary owning-cell
policy. Cancellation after irreversible transfer does not revoke an earned
fairness turn.

Every cross-lock action owns a typed fallback. Dropping an install or
cancellation action clears the cell reservation and completes the admission
operation. Dropping a candidate returns its request handle before the
connection-owning cell is advertised again. Dropping a capacity delivery
returns the permit to admission. Fallbacks run no connector, protocol, wake,
or listener code while a pool lock is held.

A fallback invoked from `Drop` may synchronously acquire a bounded sequence of pool locks to publish its
terminal state, but it holds at most one pool lock at a time. Each lock transition produces the next typed
action only after releasing the previous lock, and each action retains an idempotent fallback until the next
domain owns the terminal state. The chain performs no await, retry loop, connector or protocol work, or work
proportional to waiter or partition count, and schedules wakes or callbacks only after releasing its lock.

Pool coordination uses a crate-level synchronization facade so production and Loom tests compile the same
lock-bearing code. Production lock wrappers retain access to guarded state after standard-library poisoning so
poisoning alone cannot prevent a later `Drop` fallback from returning a permit or closing a delivery fence.
This does not make an interrupted state transition valid: code under a pool lock still preserves its
invariants without relying on poison recovery. Test builds also assert that a thread holds at most one pool
lock, turning the no-nesting rule into an executable check across the ordinary suite.

Both stay within one origin, and neither moves a driver, so the I/O-placement guarantee from
[Connection placement follows declared topology](#connection-placement-follows-declared-topology)
holds under both. This is why `max_connections_per_host` below the partition count is valid rather than an
error: a partition with no permit of its own may dispatch through an eligible
peer HTTP/1 sender or receive capacity through reclaim. The
default `NetworkInterface` scope uses both; `Partition` and `Pool` are the same machinery with a narrower or
wider eligibility group.

#### Ordering across cells

Each cell orders its own requests. Across cells, admission keeps one
origin-wide demand FIFO and separate origin and eligibility-group views over
cells with HTTP/1 connections:

```text
OriginAdmission(O)
  demand order:
    oldest -> C2/R8(H1) -> C0/R3(H2) -> C3/R5(H2) -> C1/R9(H1)

  available H1 connection-owning cells:
    origin view:       C0 -> C2 -> C3
    group eth0 view:   C0 -> C3
    group eth1 view:   C2
```

The demand order contains the current head generation from every requesting
cell waiting for origin capacity. An availability view contains a
connection-owning cell at most once while it has an H1 record that may return
or be reclaimed, has no nonterminal reuse operation, and owes no usable local
turn or older local H1 candidate. Removing a cell repairs both views
immediately, so grant work does not drain stale availability tickets.

HTTP/1 selection begins with the oldest origin demand:

```text
oldest origin demand R from requesting cell Q
  |
  +-- R accepts H1 and an eligible peer cell C exists
  |     `-- install Borrow reuse(C, Q, R)
  |
  +-- another peer H1 cell C exists
  |     `-- install Reclaim reuse(C, Q, R)
  |
  `-- no peer connection
        `-- wait for capacity, local service, or a later availability report
```

The connection selector skips the requesting cell. Same-cell idle selection
and return are resolved under that cell's lock and do not create a cross-cell
operation. Borrow takes the oldest eligible peer; reclaim takes the oldest
origin-wide peer.

The origin demand head is no younger than any eligibility-group demand head.
Selecting that origin head first therefore preserves eligible H1 ordering
without merging two demand heads. Eligibility changes only whether the
selected peer is borrowed or reclaimed.

The oldest origin demand therefore receives first use of every peer H1. If it
can borrow the selected connection, the warm connection remains open.
Otherwise reclaim closes it and returns capacity for the same oldest demand.
A younger demand in the connection's eligibility group cannot bypass the
older origin demand. One owning-cell fairness turn may follow an irreversible
transfer, bounding local overtaking without allowing peer traffic to consume
every return.

HTTP/2 peer publication uses eligibility-group demand because publication is
non-destructive and cannot satisfy an ineligible target. Those all-protocol
group views do not change HTTP/1's origin-head rule. Every cell choice remains
a stored-head operation, so the work to
grant one resource is independent of the number of cells and partitions.

#### Delivery

A released permit or provisional H1 can serve only one waiter. It must cross from admission to a cell without
being lost, copied, or left attached to a cancelled demand generation. A published H2 generation is different:
the connection record retains its permit and many compatible requests may take request leases from it. The
logical states keep these two cases separate:

```rust
enum DemandResidence {
    Idle,
    Queued {
        demand: DemandId,
        links: OrderLinks,
    },
    Delivering {
        demand: DemandId,
        delivery: DeliveryId,
        links: OrderLinks,
    },
}

enum AcquisitionPayload {
    Capacity(Permit),
    BorrowedH1 {
        reuse_id: ReuseId,
        connection_partition: PartitionId,
        candidate: ReuseCandidate,
    },
}

enum MaterializedPayload {
    Capacity(EstablishmentPermit),
    BorrowedH1 {
        reuse_id: ReuseId,
        connection_partition: PartitionId,
        selection: H1Selection,
    },
}

enum DeliveryKind {
    Capacity,
    BorrowedH1 {
        reuse_id: ReuseId,
        connection_partition: PartitionId,
    },
}

enum DeliveryAckResult {
    Accepted { successor: Option<DemandSnapshot> },
    RetrySameResidence,
    Rejected { successor: Option<DemandSnapshot> },
}

enum DeliveryGuardState {
    Undelivered {
        payload: AcquisitionPayload,
        on_drop: DeliveryAckResult,
    },
    Materialized {
        payload: MaterializedPayload,
        on_drop: DeliveryAckResult,
    },
    Disarmed,
}

struct DeliveryAck {
    delivery: DeliveryId,
    requesting_partition: PartitionId,
    successor: Option<DemandSnapshot>,
    kind: DeliveryKind,
}
```

`Queued` and `Delivering` retain the same links in origin order.
`Delivering` fences that position until the requesting cell acknowledges the
delivery, so a younger demand cannot pass a payload between lock domains. A
reuse operation does not add another demand residence: demand remains `Queued`
while reservation installation resolves and becomes `Delivering` only when a
borrowed H1 or permit is ready to cross.

One `DeliveryGuard` carries either capacity or a borrowed H1. It materializes
every fallible connection-owning-cell transition before reserving the
requesting waiter. Capacity becomes an `EstablishmentPermit`; a borrowed
candidate revalidates its owning-cell reservation and becomes an
`H1Selection`. If candidate commit fails, the guard returns the handle and
closes or retries the admission fence without changing requesting-cell state.

An owned one-to-one delivery follows this sequence:

```text
OriginAdmission lock
  Queued(R)
    -> Delivering(R, D)
    -> extract DeliveryGuard::Undelivered(payload, retry R)
unlock OriginAdmission
  |
  +-- materialize payload
  |     +-- failure -> refunnel payload; finish D; requesting cell unchanged
  |     `-- success -> DeliveryGuard::Materialized
  |
  `-- lock requesting cell
        +-- R and its oldest compatible waiter are live -> reserve waiter
        `-- stale, cancelled, satisfied, or incompatible -> reject guard
      unlock requesting cell
        `-- convert payload into acquisition event + DeliveryAck
              `-- lock requesting cell
                    +-- accepted -> waiter owns event; acknowledge D
                    `-- cancelled -> return event; refunnel and reject D
```

The admission, connection-owning-cell, and requesting-cell locks are never
nested. Between them, the delivery guard is the only payload owner. After
requesting-cell installation, `DeliveryAck` owns the admission fence and the
requesting waiter owns the establishment permit or H1 selection.

The guard makes every drop point terminal. Dropping `Undelivered` returns its raw payload before updating the
fence. Dropping `Materialized` drops the establishment permit or returns the selected H1 to its
connection-owning cell, then updates the fence. Dropping `DeliveryAck` completes its stored acknowledgement
after requesting-cell state has become authoritative. Normal execution performs the same transitions
explicitly and disarms each fallback.

`Accepted` consumes the generation and either idles the ticket or installs its successor at the applicable
tails.
`RetrySameResidence` is used only when the same generation remains useful but this payload or publication cannot
serve it; it preserves the ticket's position. `Rejected` closes the old residence after the requesting cell has
refunnelled any owned payload and carries the complete current successor, if one. A complete newer demand snapshot
may retire or replace a residence before its action reaches the requesting cell; local generation validation
then rejects the late action without resurrecting old demand.

#### HTTP/2 publication

H2 publication carries no `AcquisitionPayload`. The connection-owning record continues to own the capacity
lease, while a `(connection-owning cell, generation identity)` notice says that compatible requests may take
H2 request leases. Publishing a new local generation first installs the record and accepting generation under
the connection-owning cell lock, then makes that identity visible to compatible local waiters. They are woken
and admitted in bounded local turns; publication does not scan or synchronously wake an unbounded queue.

Publication also installs a local fairness gate:

```rust
enum GenerationGate {
    Prioritizing { cutoff: WaiterSequence },
    Open,
}
```

The cutoff is the newest compatible waiter already committed when publication becomes visible. While the gate
is `Prioritizing`, those waiters are offered generation activation oldest first in bounded turns, and a newer
arrival cannot activate through the generation ahead of them. Cancellation removes its waiter from the owed
set. When no waiter at or before the cutoff remains, the gate opens and later arrivals use the ordinary local
path. Generation invalidation removes the gate and sends any unserved committed waiter back through
acquisition. The gate orders activation opportunities; each successful activation still creates its own H2
request lease.

The peer-cell index holds one group-scoped advertisement for each accepting H2 generation. Record and
generation installation precede advertisement; transition out of accepting removes it. Either a new
advertisement or new group demand schedules a bounded publication turn from stored connection and demand heads.
The advertisement carries only connection-owning cell and generation identity, not a dispatch handle or
capacity lease. The publication guard revalidates it at the connection-owning and requesting cells. A stale
advertisement is removed or updated before the next turn, so peer H2 discovery does not scan cells.

Under bounded pressure, an accepting generation may also be announced to the
head of its eligibility-group all-protocol view. Admission fences that demand
generation in `Delivering` with a `DeliveryId`; the publication action carries
only connection-owning cell and generation identities plus an acknowledgement
fallback, never the record's capacity lease.
The requesting cell revalidates the generation identity, accepting state, demand
generation, and reuse scope.
Acceptance makes the generation visible to compatible waiters there; rejection discards the stale notice
while the generation and
permit remain at the connection-owning cell. Acceptance acknowledges after requesting-cell visibility and the
named head's activation opportunity are committed, not after every local waiter has activated. Remaining local
waiters proceed through the generation gate in bounded turns and no longer advertise a connection need while
that generation remains usable. Later group tickets are handled by subsequent bounded publication turns, so
one-to-many visibility does not turn one host action into work proportional to partition count.
Dropping a pending publication guard submits its `on_drop` acknowledgement so the fence retries or closes;
there is no single-owner payload to refunnel. Committing publication stores the requesting-cell
acknowledgement, which is submitted before the guard disarms.

This separates publication from single delivery: a permit or H1 has one owner and one requesting cell, while
an H2 generation remains owned by its connection cell and may be announced repeatedly. The transitions must
be model checked; their invariants are stated in [Correctness invariants](#correctness-invariants), with the
checks specified in [Appendix B](#appendix-b-validation).

#### Obligations

* **Bounded demand** [safety] — a cell carries at most one active demand generation regardless of how many
  requests wait, so demand accumulates no deficit and one residence receives at most one terminal outcome.
* **Snapshot ordering** [safety] — admission retains the newest complete demand version and rejects an action
  for a retired generation, so out-of-order publication cannot resurrect cancelled or satisfied demand.
* **Eligibility and capacity independence** [safety] — the capacity decision and the eligibility decision do
  not read each other's state.
* **Placement under transfer** [safety] — neither borrow nor reclaim moves a connection's driver or I/O off
  its owning partition.
* **Reclaim scope independence** [safety] — reclaim moves a permit without dispatching across a partition
  boundary, so it is not constrained by the reuse scope.
* **Single delivery** [safety] — one delivery identity owns at most one permit or provisional H1, commits it
  to at most one requesting waiter, and retains its scheduling fence until requesting-cell acknowledgement.
* **Refunnelling** [safety] — rejection, supersession, cancellation, task drop, or panic returns every
  undelivered permit to admission and every undelivered H1 to its connection-owning cell exactly once.
* **Publication ownership** [safety] — H2 publication carries generation identity, never the connection's
  capacity lease; request activation takes a request lease while the connection record remains the capacity owner.
* **Publication priority** [liveness] — publication closes the generation gate before visibility and offers
  activation to waiters committed at publication before newer arrivals, in bounded oldest-first turns.
* **Cross-cell order** [safety] — H1 borrow and reclaim both serve the current origin head, preferring an
  eligible peer connection for borrow and otherwise reclaiming an origin peer. Peer H2 publication uses its
  all-protocol eligibility-group head. Same-cell H1 service remains cell-local.
* **Reuse operation completion** [safety] — one reuse operation reserves at most one connection-owning cell and
  one requesting cell; it remains authoritative until owning-cell completion and any borrowed delivery record
  acknowledges its terminal state.
* **Return interception** [liveness] — an installed reuse reservation intercepts the next reusable H1 before it
  becomes idle, so a connection cycling continuously between active and reusable cannot strand requesting
  demand.
* **Owning-cell fairness turn** [liveness] — one irreversible cross-cell transfer creates one local turn when
  compatible local demand exists; the turn clears only when that demand is served or disappears.
* **Acknowledged progress** [liveness] — every extracted delivery or reuse action either acknowledges a
  terminal transition or executes its typed fallback, so a scheduling fence cannot remain pending solely
  because the executing future was dropped.
* **Cross-lock isolation** [safety] — admission and cell locks are never nested, and no pool lock is held
  across an await or while running connector, protocol, wake, or listener code. A synchronous fallback may
  visit a bounded sequence of lock domains but holds at most one pool lock at a time; each transition retains
  an idempotent fallback, and wakes and callbacks remain deferred until after unlock.
* **Bounded peer discovery** [optimization] — reuse and publication work select connection state from stored
  origin or group heads and validate one cell rather than scanning cells or connections. H1 availability is
  linked once and repaired eagerly; a cell publishes again only when its complete advertised or blocked state
  changes, and admission ignores reports older than its accepted availability revision.

### Liveness

A cell's queue orders its requests, and same-cell H1 service is resolved under
that cell's lock. Across cells, origin admission orders capacity demand. H1
reuse serves the current origin head: it borrows the oldest eligible peer
connection when available and otherwise reclaims the oldest origin peer.

HTTP/2 publication adds eligibility-group demand views because a generation can be announced only where it is
reusable and publication does not consume origin capacity. A terminal outcome sends any successor generation to
the applicable tails. An owning-cell fairness turn permits one local overtake after an irreversible
cross-cell transfer, but repeated reuse operations cannot keep that cell or an older peer from progressing.

Within a cell, the generation gate offers a newly published H2 generation to already committed compatible
waiters before newer arrivals. Scheduling is work-conserving among eligible waiters: if a resource a waiter
could use is free, some eligible waiter is served rather than the resource sitting idle. Each choice is a
dequeue or stored-head comparison, so the work to grant one resource does not grow with the number of waiters
or partitions.

Progress requires that a permit become reachable: an eligible connection returns reusable, an
HTTP/1 connection becomes reclaimable, or a permit is released. It is not promised while every permit for the
origin is held indefinitely by active HTTP/2 work that the waiter is not eligible to use. The pool does not
forcibly drain a live HTTP/2 connection to free such a permit; doing so would abort in-flight requests to
serve a waiter. A waiter in this state parks until eligibility or capacity changes.

#### Obligations

* **Bounded overtaking** [liveness] — a committed cell is not passed indefinitely by later arrivals; permits
  and H1 reuse use the origin-wide order, with eligible borrow preferred over reclaim for that head; peer H2
  publication uses the all-protocol group view; one owning-cell fairness turn may create only the documented
  bounded overtake.
* **Work-conserving service** [liveness] — while an eligible waiter and a resource it may use both exist,
  some eligible waiter is served.
* **Bounded grant work** [optimization] — the work to grant one resource does not grow with the waiter or
  partition count.

### Dispatching and completing a request

Acquisition ends with one request and one selected dispatch authority. Dispatch turns those into either a
terminal error or a response whose body, or upgrade path, owns the request's remaining protocol lifecycle.
This section defines that ownership transfer. HTTP framing, stream state, and flow-control behavior remain
Hyper's responsibility.

#### Preparing the request

The request future retains the original absolute URI while the request is in the pool. The origin key and
proxy decision are made from that URI before any request-target rewrite. Existing request validation and proxy
authentication behavior is preserved, including rejection of unsupported HTTP versions and HTTP/1.0
`CONNECT`, and insertion of proxy authorization only for an applicable cleartext HTTP proxy when the caller
did not supply it.

Protocol compatibility is checked against the selected connection before the request is moved into Hyper. An
HTTP/2-marked request cannot use H1; an HTTP/1.1-marked request may use H2. An incompatible H1 selection is
returned to its connection-owning cell if it remains usable, and the request receives the existing
unsupported-version error.
This applies to a fresh automatic-ALPN attempt that resolves to H1: the pool keeps the H1 for compatible
demand and does not establish repeatedly in hope of negotiating H2. The compatibility error and connection
capture both identify the H1 connection that was selected.

For H1, dispatch preserves Hyper's existing wire form. The pool inserts `Host` when configured and absent,
using the non-default port when one exists. `CONNECT` uses authority form, a request sent to a cleartext HTTP
proxy uses absolute form, and a direct or tunneled request uses origin form. H2 receives the form Hyper expects
for its codec. The retained absolute URI, not the temporary wire form, is the authority for retry, diagnostics,
and error return.

Before the Hyper call, the request's `CaptureSmithyConnection` backchannel is bound to metadata for the
selected physical connection: proxy state, local and remote addresses, and a poison callback naming that exact
connection generation. A stale-reuse retry replaces the binding with the replacement connection. The
callback is idempotent, becomes a no-op after its generation is gone, and does not keep a retired connection
or the pool alive. Connection metadata attached by the connector is also copied to the response before the
response head is exposed.

#### Readiness and Hyper acceptance

HTTP/1 readiness is a condition for entering reusable storage, not another state in request dispatch. The
request handle produced by a successful Hyper handshake may send the connection's first request. A returning
handle is not made idle or handed to a waiter until Hyper reports it ready for another request. Selection
therefore yields exclusive ownership of the handle that may call `try_send_request` directly.

Immediately before that call, the connection record commits dispatch against logical close. Dispatch commit
and logical close are mutually exclusive linearization points. If close wins, the request remains locally
owned, the stale selection retires, and acquisition runs again. If commit wins, `try_send_request` is invoked
on the same sender without publishing an intermediate state or holding a pool lock. Hyper polling,
request-body polling, callbacks, wakes, and destructive drops all occur outside pool locks.

```text
Acquired H1 sender (fresh or previously proven ready)
  -> Prepared
  -> DispatchCommit races logical close
       +-- close wins  -> retire selection; request remains unsent; reacquire
       `-- commit wins -> try_send_request on the same sender
             +-- Hyper returns original request -> UnsentReturned
             +-- Hyper accepts request ---------> WaitingForHeaders
                                                    +-- error -> TerminalError
                                                    `-- head  -> BodyGuardTransferred
```

`try_send_request` is still allowed to reject the envelope. That result is different from a pool-side stale
selection: only Hyper can certify that the original request remains unsent, and only a reused connection turns
that certification into transparent retry. A fresh connection returning the request is a terminal error.

For HTTP/2, activation of a request lease includes the generation and stream-capacity checks needed before
calling its sender. The same general boundary holds: pool state commits one dispatch before Hyper accepts the
envelope, and Hyper's returned-message behavior remains the only replay authority.

Once `try_send_request` accepts the request envelope, Hyper owns the request and its body. That point
discharges any H2 generation-gate opportunity; selecting or cloning a sender is not enough. The request future
continues to own the Hyper response future, the H1 checked-out guard or H2 receive endpoint, and a strong pool
reference while it waits for response headers; an accepted H2 request-body adapter owns the matching send
endpoint.

An accepted H2 stream has two terminal endpoints. A request-body adapter owns the send endpoint while Hyper
may still poll an upload; the request future owns the receive endpoint until it transfers that endpoint to the
response body or upgrade bridge. The request lease returns to the generation only after both endpoints are
terminal. A response arriving before a streaming upload finishes therefore cannot make the stream idle. Before
acceptance, the request-body endpoint is inert. If Hyper returns the request unsent, the pool disarms that
endpoint and can rearm the same adapter for a later selection without retaining the rejected generation.

#### Retry, timeout, and errors

There is one authority for transparent dispatch retry: Hyper must return the original, unsent request from
`try_send_request`, and the selected connection must have been reused. The pool restores the absolute URI,
disarms any unaccepted H2 body endpoint, retires or invalidates the stale selected connection, and sends that
same request through acquisition again. An unsent failure from a fresh connection is terminal and still
retires a sender Hyper reported unable to accept the request; its inert body endpoint and selected guard cannot
survive the error. The pool does not clone a request or replay one Hyper accepted.
The existing caller timeout and cancellation remain able to terminate repeated stale selections.
With Hyper 1.11, the returned-request path after `try_send_request` is reachable for H1; H2 dispatch errors
after call do not carry the original request and are terminal. An H2 readiness failure observed before call is
different: the request is still locally owned and may return to acquisition without being replayed.

Every error after Hyper accepts the envelope is terminal for this dispatch. H1 conservatively retires because
the request may have reached the wire. H2 releases or resets the affected stream and drives its send and
receive endpoints to terminal state; a stream-local reset does not by itself invalidate the accepting
generation. GOAWAY, a closed dispatcher, a connection error, or other connection-fatal evidence does
invalidate that generation. The resulting `ConnectorError` preserves the current source chain and
classification, including timeout, user, I/O, incomplete-message transient,
GOAWAY, and `REFUSED_STREAM` behavior.

The read timeout keeps its current scope: `PoolConnector::call` starts it around the client request operation,
including acquisition, establishment when needed, dispatch, and the wait for response headers, and ends it
when headers arrive. It does not time body reads or an upgraded stream. A connect timeout covers only the
connector operation for one establishment attempt; a participant waiting on another request's HTTP/2 flight
has no connector operation to time. When either timeout wins, dropping the operation follows the cancellation
rule for its current dispatch stage; timeout classification does not bypass lifecycle cleanup.

#### Response and cancellation ownership

Before returning response headers, the request future installs the response lifecycle guard and response
metadata. That transfer is atomic from the caller's perspective: after a successful call, the returned body
or upgrade path owns cleanup; before it, the request future does. A panic or cancellation cannot land between
the two with no owner.

| Stage               | Request                   | Dispatch handle                               | Connection / stream guard                                              | Response body            | Retry authority        |
| ------------------- | ------------------------- | --------------------------------------------- | ---------------------------------------------------------------------- | ------------------------ | ---------------------- |
| Acquiring           | Request future            | None                                          | Waiter or delivery fallback                                            | None                     | None                   |
| Prepared / selected | Request future            | Selected sender                               | H1 checked-out guard or prospective H2 lease                           | None                     | None                   |
| Unsent returned     | Request future regains it | Stale sender retires                          | Guard resolves; H2 endpoint is inert                                   | None                     | Reused connection only |
| Accepted / headers  | Hyper                     | H1 request future; H2 local handle releasable | H1 request future; H2 send and receive endpoints                       | None                     | None                   |
| Headers delivered   | Consumed                  | H1 body guard; no H2 local handle             | Body or upgrade owns H1 or H2 receive; request adapter may own H2 send | Caller owns guarded body | None                   |
| Terminal            | None                      | H1 owning cell or retired; H2 generation      | H1 returned or closing; H2 lease released                              | Completed or dropped     | None                   |

The open connection record owns its capacity lease throughout this table. Dispatch never moves the permit into
the request, sender, body, or request lease; only logical close returns it to admission.

Dropping during acquisition uses the waiter, delivery, and refunnelling rules already defined. Dropping after
selection but before call returns a still-usable H1 through its connection-owning cell's ordinary return path
or releases the H2 request lease. Dropping after Hyper accepts but before headers closes H1 through Hyper's supported
cancellation path; on H2 it resets only the stream with `CANCEL`, terminates the receive endpoint, and lets
the request-body adapter terminate the send endpoint before releasing the request lease. Dropping
after headers follows the body rules in [Returning a connection](#returning-a-connection).

Poisoning is monotonic and generation-specific. Invoking captured metadata's poison callback immediately
removes the named H1 record or H2 generation from new dispatch, but does not abort an accepted request merely
to accelerate replacement. An active H1 begins logical close immediately and tears down after its exchange
reaches a terminal boundary. H2 stops accepting new leases while existing streams drain. A concurrent driver
error, GOAWAY, idle timeout, reclaim, or repeated poison races through the same exactly-once logical-close
transition.

#### Upgrades

An upgrade changes which object owns protocol completion. H1 drivers run with upgrade support. A `101`
response or successful HTTP/1 `CONNECT` logically closes the checked-out record before the response is
exposed: its sender cannot return to the pool, and bounded capacity is released immediately. There is no
separate upgrade-pending pool residence.

Hyper's upgrade-capable driver owns the subsequent transport transfer. It moves the wrapped transport and any
bytes read past the HTTP message into the `Upgraded` object. The caller then owns that I/O; dropping it signals
physical completion through the transport wrapper. If `OnUpgrade` is dropped or the transfer fails, Hyper
closes the transport instead.

Hyper may complete its HTTP/1 driver in the same poll that delivers the upgrading response head. The driver
guard can therefore record `ProtocolClosed` before the request task observes the response. Once the response
path confirms `101` or successful `CONNECT`, it refines that close reason to `Upgraded`; the refinement changes
no ownership and cannot release bounded capacity again. In either poll order the H1 record was already
logically closed and can never return as an HTTP connection.

For H2 extended `CONNECT`, the physical H2 connection remains pooled but that stream is no longer represented
by an ordinary response body. An upgrade lifecycle bridge takes the response's receive endpoint and retains it
with the upgraded stream until both upgraded directions are terminal. The owner-partition executor may attach
the bridge to Hyper's `UpgradedSendStreamTask`, but task completion alone is sufficient
only when it proves the receive half is also done; otherwise the Hyper integration needs a narrow full-stream
completion hook. The original request-body send endpoint must also be terminal before the lease releases.
Releasing the lease resets or completes that stream only. Other streams and future requests
may continue on the same accepting generation. Ordinary response-body completion must not release the
transferred lease early.

#### Obligations

* **Same-instance dispatch** [safety] — one selected H1 sender commits against logical close and calls
  `try_send_request` directly, without publishing or transferring an intermediate dispatch state.
* **Wire-form restoration** [safety] — temporary request-target rewriting never replaces the retained absolute
  URI used for retry, diagnostics, or errors.
* **Certified retry** [safety] — transparent retry uses only the original request Hyper returned unsent from a
  reused connection; an accepted request and a fresh-connection failure are never replayed.
* **Continuous response ownership** [safety] — the request future owns cleanup through response headers or
  terminal error, then transfers it to the response body or upgrade bridge before exposing the response.
* **Stage-local cancellation** [safety] — cancellation returns or retires H1 according to whether Hyper
  accepted it, and releases or resets only the selected H2 request lease.
* **Compatibility surface** [safety] — request validation, target form, proxy authentication, metadata,
  timeout scope, source chain, and error classification preserve the current client behavior.
* **Upgrade transfer** [safety] — an H1 upgrade transfers root I/O and cannot return to the HTTP pool; an H2
  extended `CONNECT` transfers its request lease to an upgrade bridge until both stream directions terminate.
* **Full-stream lease** [safety] — an accepted H2 request releases its request lease only after both its
  request-send and response-receive endpoints are terminal.
* **Poisoned generation** [safety] — poisoning prevents every later dispatch on the named record or generation
  without aborting already accepted work solely for replacement.

### Connection retirement and maintenance

After an accepted request reaches a terminal protocol boundary, its connection either returns to service or
leaves the pool. This section defines that decision and the two-step close that retires a connection.

#### Returning a connection

Response headers do not make a connection reusable. The guarded body owns H1 lifecycle or the H2 receive
endpoint until the response reaches end-of-stream, fails, or is dropped.

For H1, end-of-stream begins return processing; the checked-out sender returns to its owning cell only after Hyper
also reports it ready for another request. Dropping an incomplete body is not itself evidence of reusability.
Hyper may synchronously consume an already-buffered remainder and prove the message boundary; if it does, the
same ready check may return the connection. If the remainder is unavailable, cancellation, body error, or
protocol state cannot prove the boundary, the connection logically closes. The pool does not parse or drain
HTTP independently of Hyper.

The response path polls H1 readiness once. If readiness is pending after the response reaches a reusable
protocol boundary, it transfers an `H1Exchange` into a readiness task spawned through the connection's
owner-partition `DriverSpawner`. The response body does not retain responsibility for polling
that sender, and `Drop` never waits. The task enters owning-cell return only after Hyper proves both the message
boundary and readiness for another request. Closed, poisoned, upgraded, or owner-runtime-shutdown outcomes
logically close the record; dropping the task owns the same connection-close fallback.

For H2, body end-of-stream or a stream-local error terminates the receive endpoint. Dropping an incomplete
body does the same and asks Hyper to send `RST_STREAM(CANCEL)`; the lease releases after the
request-body send endpoint also terminates. Neither outcome retires an otherwise healthy generation. GOAWAY,
connection failure, or explicit poisoning may independently have moved the generation to draining, in which
case the last lease completes drain instead of returning it to accepting
state. An H2 extended `CONNECT` follows its upgrade lifecycle bridge rather than the ordinary body terminal.

Every H1 return revalidates the record's generation, poison state, and idle
policy under the connection-owning cell lock. An unbounded origin serves
compatible local demand or installs the connection as idle directly because it
has no admission state or cross-cell reuse. For a bounded origin, the same
transition also checks its installed reuse reservation. An installed
reservation extracts the sender into a provisional candidate for borrow or
reclaim. Without a reservation, the owning cell first serves compatible local
demand and otherwise installs the sender as idle. This complete decision is
cell-local; a returning sender does not synchronously consult admission.

After the cell transition, a bounded connection-owning cell publishes an
availability change only when its complete advertised or blocked state changed.
Demand-driven admission may then install a future peer reuse operation, but it
cannot interpose between the just-completed local return decision and its sender ownership. `Reserved` is
counted as active rather than idle because no request may select it. Every transition revalidates retirement
state, so a body that finishes concurrently with poison, reclaim, driver failure, or pool shutdown cannot
republish a connection after retirement.

#### Two-phase close

A connection that leaves the pool does so in two steps. At **logical
close** the connection stops accepting new work and releases its permit; at **physical close** the socket is
gone. The permit returns to admission at the first step, not the second, so a replacement can be admitted
while the old transport is still finishing its teardown.

Releasing capacity at logical close is what keeps a slow teardown from stalling the pool. A connection's
socket does not close instantly: TLS sends `close_notify`, TCP exchanges FIN, and the OS may linger the
socket after that. Were the permit held until the socket was gone, a connection ending would block a waiter
for the length of a teardown the pool does not control. Instead the permit is released the moment the
connection stops taking work, and the driver finishes the teardown on its owning partition afterward.

The complete connection lifecycle is:

```text
establishing (attempt or flight owns capacity lease)
  |
  +-- failure/cancel -------------------------------> permit refunnelled; no connection
  |
  `-- handshake -> record owns capacity lease; guarded driver task armed
        |
        +-- H1 open/idle <---- successful return ---- H1 checked out
        |       |                                      |
        |       |                                      +-- response body complete
        |       |                                      |     `-- Hyper ready -> return
        |       |                                      +-- incomplete body dropped/error
        |       |                                      |     +-- Hyper proves boundary -> return
        |       |                                      |     `-- otherwise -> logical close
        |       |                                      `-- upgrade -> logical close + transfer I/O
        |       |
        |       `-- idle/reclaim/poison/driver close --------> logical close
        |
        `-- H2 accepting generation
                |
                +-- take request lease -> request-send + response-receive endpoints
                |                         `-- both terminal -> release request lease
                |
                `-- GOAWAY/poison/driver close -------------> logical close

logical close (once: remove reuse eligibility + release capacity lease)
  |
  +-- no accepted work ------------------------------> transport teardown
  +-- H1 accepted exchange --------------------------> finish or cancel, then teardown
  +-- H2 request leases remain ----------------------> drain to zero, then teardown
  `-- H1 upgraded I/O transferred ------------------> caller owns wrapped transport
                                                        |
transport root is dropped <-----------------------------+
  `-- physical completion
```

The consequence is that live sockets can outnumber admitted connections: a replacement admitted at logical
close coexists with a victim not yet physically closed. `max_connections_per_host` bounds admitted
connections; **no finite general bound on live sockets follows from it.** How long a socket lingers after
logical close depends on peer and OS behavior, and a busy origin can keep admitting replacements while earlier
sockets are still draining, so the count of physically-live sockets for one origin has no bound expressible in
`N` alone. A caller who needs a file-descriptor ceiling sets it at the OS, not through this option. Accepted
H2 streams that outlive their connection's logical close are draining, not admitted: they hold no permit and
accept no new requests, and they too close on their own schedule.

#### Why a connection retires

The reason recorded by the logical-close transition is part of the observation surface:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CloseReason {
    IdleTimeout,
    Poisoned,
    ProtocolClosed,
    IncompleteH1Exchange,
    Upgraded,
    Reclaimed,
    PoolDropped,
    OwnerRuntimeShutdown,
}
```

A connection retires for one of a few reasons:

* **Idle timeout** — an H1 deadline starts when its sender becomes reusable in the idle set and is absent while
  the record is selected, active, or resolving return. A new H1 therefore begins aging only after its first
  idle installation. An H2 deadline starts when a generation becomes accepting, including a fresh generation
  that has not dispatched, and resets whenever a request lease commits to dispatch. Active streams do not
  suspend that deadline. H2 expiration moves the generation out of accepting state and begins logical close;
  accepted leases continue draining and retain the physical transport. A connection with idle timeout disabled
  is kept.

  Closing an otherwise-quiescent connection cannot wait for the next request, so each partition runs a
  maintenance task on its own runtime. Every task uses the pool's builder-injected `TimeSource` and
  `AsyncSleep`, so tests drive idle age with a fake clock. Because that time source is `SystemTime`, which can
  step backward or forward, idle age is measured against the scheduled sleep deadline the task already holds,
  not by subtracting two `SystemTime` readings. The deadline gives a monotonic floor: a clock jump changes when
  the task wakes but not whether a connection idle since a completed deadline has expired.

  The scheduler records the deadline represented by its current sleep. A newly idle connection wakes it only
  for an earlier deadline; ordinary checkout does not force a partition scan. An atomic start gate submits at
  most one task per partition. The task retains only weak cell registrations between scans, drops each strong
  scan snapshot before waiting, and exits on explicit partition shutdown even when no cell or deadline remains.
  At the start of a scan, it atomically retires the deadline that triggered the scan while capturing the
  scheduler revision. A connection returned to idle during the unlocked scan therefore advances the revision
  and forces a retry rather than being hidden behind an already elapsed deadline. Shutdown and earlier-deadline
  publication detach the waker under the scheduler lock and wake it after unlock. If the submitted maintenance
  future is dropped before normal completion, its task guard reopens the start gate so a later request can
  submit maintenance again.

* **Poisoning** — an explicit poison signal through captured connection metadata removes the named record or
  generation from future dispatch. Accepted work may finish, but the connection does not return to accepting
  or idle state.
* **Protocol close** — peer close, HTTP/2 `GOAWAY`, driver or dispatcher completion, and transport- or
  protocol-level connection failure retire the affected connection. Streams the peer still accepts may
  finish; later requests use a replacement. An H2 stream-local reset is not connection-fatal.
* **Incomplete H1 exchange** — cancellation, response-body drop, or an exchange error retires H1 when no
  independent connection-fatal signal has already selected `ProtocolClosed`, unless Hyper proves that it
  recovered the complete message boundary.
* **Upgrade** — an H1 connection leaves HTTP pool ownership when its transport transfers to the upgraded
  protocol. H2 extended `CONNECT` follows the request stream's upgrade bridge and does not retire the physical
  H2 connection.
* **Reclaim** — a bounded origin at its limit closes a connection to move its permit to a waiting cell, as
  [Bounded-capacity coordination](#bounded-capacity-coordination) describes. Reclaim never interrupts an in-flight
  request: it closes a connection that is idle now, or reserves one as it returns from its current request and
  closes it before it serves another. It does not abort active work to free a permit.
* **Pool or owner-runtime shutdown** — pool drop logically closes every remaining record. If an owning runtime
  drops a connection's guarded driver task, the driver lifecycle guard requests logical close with
  `OwnerRuntimeShutdown`; dropping the driver also closes its root transport unless an H1 upgrade already
  transferred that transport. Per-stream and upgrade tasks use their request-lifecycle cleanup and do not by
  themselves close a healthy H2 connection. Outstanding request and body guards run their normal terminal
  cleanup, and every close request races through the same exactly-once transition.

The first trigger to begin logical close removes reuse eligibility, releases capacity, and records the close
reason. Later triggers observe that terminal transition and cannot release capacity or report close again. The
one reason-only refinement is an H1 upgrade: Hyper can complete the protocol driver in the same poll that
delivers the upgrade response, so a request path that later confirms the upgrade may change
`ProtocolClosed` to `Upgraded` without repeating close, capacity release, or the close callback. The
refinement emits a structured diagnostic record carrying both reasons so tracing reflects the final
classification even when driver completion won the close race.

`Poisoned` is reserved for an explicit poison signal. `ProtocolClosed` is final when it reflects independently
observed connection-level termination; only later confirmation that the same H1 exchange upgraded may refine
it. The close event carries the source error when one exists. Other concurrent signals still race through
first-trigger-wins, but one initiating signal does not match both categories. Every reason ends at the same
physical completion, so capacity and lifecycle accounting do not depend on what won the race.

#### Obligations

* **Capacity on logical close** [safety] — a connection releases its permit at logical close, before physical
  close, and releases it exactly once.
* **No dispatch after logical close** [safety] — dispatch commit and logical close race through mutually
  exclusive per-record or per-generation gates; a close that wins leaves the request locally owned, while a
  commit that wins is recorded as an in-flight dispatch and calls Hyper immediately without holding a pool
  lock.
* **Poison on retire** [safety] — a connection retired as unsafe is not returned to the idle set.
* **Reclaim spares active work** [safety] — reclaim closes a connection only when it is idle, whether idle
  now or on return from its current request; it never interrupts an in-flight request.
* **H1 boundary return** [safety] — H1 returns only after response end-of-stream or a Hyper-proven drain and a
  successful sender-ready check; pending readiness is owned by an owner-partition task, and dropping a body
  alone never returns the connection.
* **H2 stream isolation** [safety] — completion, error, or cancellation releases or resets one H2 request
  lease, after both endpoints terminate, without retiring a healthy accepting generation.
* **Return revalidation** [safety] — an H1 return checks generation and retirement state under its
  connection-owning cell before becoming visible; a sender awaiting admission remains owned by that cell and
  non-dispatchable in `Reserved`, so a late completion cannot reverse logical close or bypass return ordering.
* **Physical completion tracking** [safety] — root-I/O drop, not logical close or driver-future completion
  alone, terminates the physical connection lifetime, including after H1 upgrade.

### Telemetry

An operator diagnosing a connection problem — establishment happening more often than expected, connections
closing early, reuse not occurring — cannot see it in request outcomes, which look the same whether a request
reused a warm connection or opened a fresh one. The pool reports what request outcomes hide, in two forms.

#### Events and statistics

Lifecycle **events** report transitions; statistics report current gauges. Their public types live here
because their fields derive directly from the connection states above:

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConnectionId(/* private */);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NegotiatedProtocol { Http1, Http2 }

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ConnectionInfo {
    pub id: ConnectionId,
    pub origin: OriginKey,
    pub owner_partition: PartitionId,
    pub protocol: NegotiatedProtocol,
    pub local_addr: Option<SocketAddr>,
    pub remote_addr: Option<SocketAddr>,
    pub proxied: bool,
}

#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct ConnectionTiming {
    // Includes DNS, TCP, proxy, and TLS work performed by the connector.
    pub connector: Duration,
    pub protocol_handshake: Option<Duration>,
}

#[non_exhaustive]
pub struct ConnectionCreatedEvent {
    pub connection: Arc<ConnectionInfo>,
    pub timing: ConnectionTiming,
}

#[non_exhaustive]
pub struct ConnectionReusedEvent {
    pub connection: Arc<ConnectionInfo>,
    pub request_partition: PartitionId,
}

#[non_exhaustive]
pub struct ConnectionBorrowedEvent {
    pub connection: Arc<ConnectionInfo>,
    pub request_partition: PartitionId,
}

#[non_exhaustive]
pub struct ConnectionClosedEvent {
    pub connection: Arc<ConnectionInfo>,
    pub reason: CloseReason,
    pub error: Option<BoxError>,
}

#[non_exhaustive]
pub struct ConnectionFailedEvent {
    pub origin: OriginKey,
    pub partition: PartitionId,
    pub protocol: Option<NegotiatedProtocol>,
    pub remote_addr: Option<SocketAddr>,
    pub timing: ConnectionTiming,
    pub error: BoxError,
}

pub trait ConnectionEventListener: Send + Sync + 'static {
    fn connection_created(&self, _event: &ConnectionCreatedEvent) {}
    fn connection_reused(&self, _event: &ConnectionReusedEvent) {}
    fn connection_borrowed(&self, _event: &ConnectionBorrowedEvent) {}
    fn connection_closed(&self, _event: &ConnectionClosedEvent) {}
    fn connection_failed(&self, _event: &ConnectionFailedEvent) {}
}
```

The connection record and its events share one immutable `Arc<ConnectionInfo>`, so reporting reuse does not
reallocate origin or address metadata. `ConnectionId` is unique within one pool and is never reused.
Installed-connection events carry `ConnectionInfo`, whose `owner_partition` is where the physical I/O and
driver live; the separate `request_partition` on reuse and borrow reports where demand originated.
`ConnectionBorrowedEvent` reports a successful H1 transfer from a connection-owning cell to a requesting cell.
Cross-partition H2 selection is visible as reuse whose owner and request partitions differ, preserving
*borrow* as the H1 mechanism defined above.
`ConnectionFailedEvent` is different: an attempt can fail before a physical connection identity or negotiated
protocol exists, so it identifies the origin and attempted partition and makes protocol optional.

The emission points follow ownership transitions rather than future creation or object drop:

```text
establishment accepted
  |
  +-- connector or handshake fails
  |     -> release establishment accounting
  |     -> connection_failed
  |
  `-- driver transferred + record installed + physical metadata fixed
        -> update connection accounting
        -> connection_created
        -> publish H1 record or H2 generation to requests
              |
              +-- existing connection selected -> connection_reused
              +-- cross-cell H1 transfer commits -> connection_borrowed
              `-- first logical-close trigger
                    -> remove eligibility + release admission + record reason
                    -> connection_closed exactly once
                          `-- later root-I/O drop updates physical statistics only
```

Creation is the one event-order barrier: its synchronous callback returns before the connection becomes
request-visible. A callback panic instead unwinds the establishment task and guarded cleanup prevents the
pre-published record from becoming usable. After publication, callbacks from concurrent tasks are not
serialized. A reuse callback may still be running when another task begins a close callback, and no ordering
is promised between two concurrent reuses. Events causally emitted by one task are invoked in that task's
program order until a callback panics. This is the complete ordering contract; consumers needing a total order
add timestamps or sequencing in their listener.

`connection_reused` is emitted when an existing H1 guard or H2 request lease is committed to a request, before
Hyper readiness. A stale selection may therefore be followed by `connection_closed` and a transparent retry;
the event reports the attempted reuse that operators need to diagnose. `connection_closed` marks logical
close, not physical teardown, and reports the reason that won that transition. If Hyper's H1 driver wins a
race with upgrade confirmation, the callback reports `ProtocolClosed` exactly once and the later one-way
diagnostic refinement records `ProtocolClosed -> Upgraded` without another callback. If the request path wins,
the callback reports `Upgraded` directly. Establishment failure is emitted after its capacity and waiter state
have been reconciled. All callbacks observe committed pool state.

The same `ConnectionInfo` remains available through the existing per-request connection-capture API. Events
extend observation; they do not replace metadata capture or its generation-specific poison callback.

Statistics use the same state boundaries:

```rust
#[derive(Clone, Copy, Debug, Default)]
#[non_exhaustive]
pub struct PartitionStats {
    pub establishing: usize,
    pub h1_idle: usize,
    pub h1_active: usize,
    pub h1_draining: usize,
    pub h2_accepting: usize,
    pub h2_draining: usize,
    pub h2_active_streams: usize,
    pub physically_live: usize,
    pub waiting_requests: usize,
}

pub struct OriginStats { /* sparse per-partition snapshot */ }

impl OriginStats {
    pub fn limit(&self) -> Option<usize>;
    pub fn admitted(&self) -> usize;
    pub fn get(&self, partition: PartitionId) -> Option<PartitionStats>;
    pub fn iter(&self) -> impl Iterator<Item = (PartitionId, PartitionStats)> + '_;
    pub fn is_empty(&self) -> bool;
}

impl ConnectionPool {
    pub fn stats(&self, origin: &OriginKey) -> OriginStats;
}
```

`establishing` starts when an attempt or flight is admitted and ends when it fails or installs a record.
`h1_idle` and `h1_active` partition logically open H1 records; checked-out and `Reserved` H1 records are
active, while only dispatch-eligible records in the idle set are idle.
`h2_accepting` counts generations that may issue request leases, while `h2_active_streams` counts accepted
request leases across accepting and draining generations. Logical close moves a connection out of those
admitted gauges and, while its transport remains, into `h1_draining` or `h2_draining`. An upgraded H1 remains
H1-draining until its wrapped root I/O drops. `physically_live` starts when the connector's returned transport
is wrapped for
lifecycle tracking and ends only at root-I/O drop, so it includes handshaking, admitted, draining, and
upgraded transports and may exceed the configured limit. `waiting_requests` counts requests registered in
acquisition that do not yet own a dispatch authority, including flight followers.

`OriginStats::admitted` derives the saturating sum of `establishing`, `h1_idle`, `h1_active`, and
`h2_accepting` from the captured partition rows; it is not another shared counter or admission authority.
Draining connections are excluded because they hold no permits. `limit` is the configured per-origin bound,
or `None` for an unbounded origin. A connection and its streams are attributed to the owner partition even
when another partition dispatches through them; a waiting request is attributed to its requesting partition.

Each gauge changes at its named transition before any corresponding callback. The counters use relaxed or
otherwise non-coordinating reads so observation adds no origin-wide write to a local reuse hit. Consequently,
an `OriginStats` value is not an atomic multi-counter snapshot: a transition can appear in one loaded field
and not yet in another, so `admitted` can transiently over- or under-report the authoritative state even
though it always equals its captured rows. Each field is nonnegative and converges to the state after
concurrent transitions settle. Statistics are diagnostic values, not admission authority or a synchronization
API.

The result is sparse: only partitions whose stable cell for the origin has been created appear, including
`PartitionId::ANONYMOUS` for the default pool. An unknown but valid origin returns an empty result. These are
current gauges, not cumulative operation counts; rates and close-cause totals belong in an event listener.
Events answer "what happened" while statistics answer "what state is represented now."

#### The listener contract

A listener runs synchronously, on the request or task that produced the event, and outside every pool lock.
Running outside locks is what lets the pool call a listener without freezing pool coordination; running
synchronously means a slow listener delays the request, establishment, maintenance, or driver task that
triggered it and may defer work that follows that transition. A listener must not block on or wait for pool
work that depends on the invoking task.

A panicking listener does not corrupt the pool. The pool completes its state change and releases its locks
before it invokes a listener, so a panic unwinds only the task that triggered the event — that one request
fails — and leaves pool invariants intact. The pool does not catch the panic or guarantee delivery of the
events that would have followed on the same task; a listener is an observer, and containing its own panics is
the caller's responsibility.

#### Obligations

* **Report locality** [safety] — a listener is invoked outside all pool locks, after the triggering state
  change is complete.
* **Panic containment** [safety] — a panicking listener leaves committed pool state and invariants intact;
  guarded cleanup resolves any follow-up transition that the invoking task had not yet published.
* **Creation before visibility** [safety] — an installed connection invokes its created callback before it is
  visible for request selection; a callback panic cannot publish the pre-created record.
* **Single close report** [safety] — logical close records one reason and invokes at most one close callback
  for a connection identity. The only later reason mutation is the one-way H1-upgrade refinement from
  `ProtocolClosed` to `Upgraded`; it changes no ownership, capacity, close count, or callback count and emits
  both the previous and refined reasons to structured tracing.
* **Attribution** [safety] — every installed-connection event identifies the physical connection, owning
  partition, and negotiated protocol; a failed attempt identifies its origin and attempted partition.
* **Defined gauges** [safety] — every statistics field changes only at its named lifecycle transition and is
  never used as admission or scheduling authority.

## Terminology

Terms whose everyday meaning would otherwise mislead. Everything else is defined where it is first used.

**Partition** — an establishment/driver placement and optional network-interface binding, identified by a
caller-owned `PartitionId` when explicit or `PartitionId::ANONYMOUS` in the default topology. An explicit
partition names its runtime at construction; the anonymous partition binds one on first establishment.

**Origin** — a canonicalized scheme, host, and port, `OriginKey`. The granularity at which connections are
interchangeable. Discovered at runtime.

**Cell** — one partition's connections for one origin, `OriginCell`. The two axes' intersection, and where
connections live.

**Connection-owning cell** and **requesting cell** — roles in cross-cell reuse. The connection-owning cell
retains the record, driver, socket, and placement. The requesting cell owns the demand that may borrow the
HTTP/1 request handle or receive reclaimed capacity.

**Permit** — the conserved unit of connection capacity. It has exactly one owner at a time and is moved or
released, never copied. *Capacity* is the aggregate quantity permits account for, used in sums and bounds.

**Demand** — a cell's standing signal that it could use one more connection. One fixed ticket per cell,
not one per request, so demand cannot accumulate.

**Borrow** — moving an exclusive HTTP/1 request handle to a peer cell for dispatch while leaving the
connection record, driver, and socket with the connection-owning cell.

**Reclaim** — closing a connection so its permit can move to another cell. Transfers capacity, not I/O.

**Capacity lease**, **request lease**, and **handle** — a capacity lease is the exclusive hold on one permit
and moves from establishment to the connection record. An H2 request lease owns one prospective or accepted
stream's two-ended lifecycle but no permit. A dispatch handle can address a connection and owns neither kind of
capacity.

**Retry authority** — proof that the same request may be dispatched again. Only Hyper returning the original
request unsent from a reused connection creates this authority; request clonability does not.

**Publish** and **deliver** — publication makes state visible to many unnamed readers; delivery hands one
value to one waiting party. `DemandResidence` tracks a ticket's residence and acknowledgement fence;
`DeliveryGuardState` owns a one-to-one payload while it crosses locks.

**Attempt** and **flight** — an HTTP/1 establishment is an attempt, independent of other attempts; an
HTTP/2 establishment is a flight. Automatic attempts remain independent before ALPN, then atomically join or
install at most one post-ALPN H2 flight per cell.

**Logical close** — a connection stops accepting new work and releases its permit. **Physical close** —
the socket is gone. Physical close follows logical close by an unbounded interval, so live sockets can
outnumber admitted connections; `max_connections_per_host` bounds admitted connections, not file descriptors,
and no finite bound on live sockets follows from it.

**Draining generation** and **draining connection** — a draining H2 generation accepts no new request leases
while already accepted streams finish. A draining connection has logically closed and released its permit but
still has physically live root I/O, and is counted by `h1_draining` or `h2_draining`. An H2 record may be both
while its accepted streams and transport finish.

**Generation** — an HTTP/2 connection's dispatch epoch, a first-class object with a lifecycle.

**Demand generation** — one cell queue head's `DemandId`. A **snapshot version** orders complete publications
for that generation. Readers retain the newest publication and reject work for a retired generation.

**Obligation** — a duty a component owes, stated as one sentence an implementation either satisfies or does
not. `[safety]` obligations forbid a state; `[liveness]` obligations require an outcome; `[optimization]`
obligations bound a cost, and violating one is a regression rather than a defect.

## Correctness invariants

Each invariant states the property, what it rules out, and the obligations that enforce it. The obligations
themselves are defined in the Architecture sections; an invariant names them rather than restating them.
Optimization obligations are not invariants — violating one is a regression, not a defect — so they appear
here only where a cost bound is load-bearing for a correctness property.

**Capacity is conserved.** A bounded origin admits at most `max_connections_per_host` connections across all
partitions, and every permit has exactly one owner at all times. *Rules out:* admitting past the bound; a
leaked permit, which is capacity the pool can never reissue; a duplicated permit, which would let two
connections occupy one unit of capacity. *Enforced by:* Single permit owner, Driver termination closes the
record, and Capacity on logical close (one owner from admission through release, released exactly once),
Single delivery and Refunnelling (a permit crossing locks commits once or returns to admission), and
Losing-attempt cleanup and Flight cancellation (post-ALPN termination returns the establishment lease).
Capacity gates whether a new connection may be *established*; it does not gate dispatch on a connection
already admitted.

**Origins are total and canonical.** Every dispatched request maps to exactly one origin, and equivalent
spellings of one server map to one origin. *Rules out:* a request with no cell to resolve to; one server
splitting into two `OriginAdmission`s that each admit the full bound. *Enforced by:* Key totality,
Canonical key, and Version independence.

**Cell identity is stable.** A cell is never destroyed while its origin is reachable, and at most one cell
exists per (partition, origin). *Rules out:* a peer reference to a cell that has been freed or whose slot has
been reused; two cells racing into existence for one pair. *Enforced by:* Cell stability, Reference validity,
and Cell uniqueness.

**I/O stays on its owning partition.** A connection's socket, driver, and Hyper-spawned tasks run only on the
partition that established it, for the connection's life; reuse moves a dispatch handle and no I/O. *Rules
out:* a socket registered on one runtime and driven from another, which produces a cross-runtime wakeup on
every read and couples the connection's lifetime to a runtime that does not own it; bytes leaving an
interface the caller did not choose. *Enforced by:* Establishment placement, Driver placement, Hyper task
placement, Binding immutability, and Placement under transfer.

**No dispatch on an unusable connection.** No request is dispatched on a connection that has begun closing or
has been retired as unsafe. *Rules out:* use of a connection after logical close; drawing a connection a
prior error poisoned. *Enforced by:* No dispatch after logical close, Poison on retire, Reclaim spares active
work (reclaim closes a connection only while idle, never interrupting an in-flight request), Poisoned
generation, Return revalidation, and Same-instance dispatch.

**Dispatch and response ownership are continuous.** From selection through terminal response, exactly one
component owns the request and exactly one component owns the selected connection or stream cleanup. *Rules
out:* replaying a request that may have reached the wire; returning H1 while its response is still framed;
losing a checked-out connection or H2 request lease when a future is dropped; releasing an extended
`CONNECT` lease when its empty response body completes. *Enforced by:* Same-instance dispatch, Certified
retry, Continuous response ownership, Stage-local cancellation, Upgrade transfer, H1 boundary return, H2
stream isolation, Full-stream lease, and Return revalidation.

**A one-to-one resource is delivered exactly once.** A provisional H1 or capacity lease has one owner until it
is committed to one eligible waiter or refunnelled. *Rules out:* a lost resource while an eligible waiter
sleeps; a double delivery where one resource serves two waiters; a cancelled waiter retaining capacity.
*Enforced by:* Bounded demand and Snapshot ordering identify the live generation; Single delivery retains its
fence through acknowledgement; Refunnelling and Acknowledged progress give every rejection and drop a terminal
path. An H2 generation is not a one-to-one resource: its connection record retains capacity while generation
identity is published to compatible local waiters and announced to eligible peer cells in bounded turns.

**No committed waiter starves.** An eligible committed waiter is served whenever a permit it may use becomes
reachable, and is not passed indefinitely by later arrivals. *Rules out:* unbounded overtaking; a resource
sitting idle while an eligible waiter waits; capacity stranded on a peer connection that returns reusable
without ever going observably idle; a newly published H2 generation serving newer local arrivals while older
compatible waiters remain parked. *Enforced by:* Cross-cell order and Bounded overtaking (the oldest eligible
residence comes from a stored head), Return interception and Owning-cell fairness turn (a returning connection
reaches an older peer without starving its owning cell), Publication priority (the generation gate serves committed
local waiters before newer arrivals), and Work-conserving service, with Bounded grant work bounding the
coordination cost and Bounded peer discovery preventing peer searches from growing with partition count.
This holds only while progress is possible — it is not promised while every permit for the origin is held
indefinitely by active HTTP/2 work that the waiter is not eligible to use, a limit stated under
[Eligible requests make progress](#eligible-requests-make-progress).

**Observation cannot corrupt pool state.** Listener code runs outside pool locks and only after its triggering
transition is complete, so pool invariants do not depend on a listener succeeding. *Rules out:* a listener
observing or holding partially transitioned state; a panicking listener leaving committed state inconsistent;
a listener blocking coordination by retaining a pool lock. *Does not rule out:* a listener delaying or
ending the task that invokes it, or delaying work sequenced after its return. The creation callback is a
barrier before request visibility. *Enforced by:* Report locality, Panic
containment, and Creation before visibility.

## Future work

### Reclaiming quiescent origins

The initial pool retains each origin and its cells until pool drop. Retained route memory therefore grows with
partitions × origins ever touched even after the connections and waiters for those origins are gone. Keeping
the identities stable is safe: it preserves peer references and guarantees that a bounded origin has exactly
one admission authority, with no reclamation race on the local acquisition path.

Whole-origin reclamation is the viable future granularity because nothing outside an origin refers to one of
its cells. It still needs a protocol that makes a request which has resolved the old origin visible before a
concurrent removal can declare every cell quiescent; otherwise old and replacement admissions can coexist and
each admit the full bound. Revisit this after measuring the retained size of an empty cell and realistic
partition-by-origin cardinality. A cell-count ceiling is not an alternative because it converts memory growth
into request failure.

### Active HTTP/2 drain for cross-scope reclaim

A waiter may remain parked when an origin is bounded, every permit is held indefinitely by active H2
generations outside that waiter's reuse eligibility group, and no eligible connection, reclaimable H1 return,
or released permit appears. This requires sustained cross-scope H2 work, such as different network-interface
groups, and is visible through waiting, H2-active-stream, and per-partition admission statistics.

The initial pool does not forcibly drain active H2 work to recover that capacity. A future implementation could
mark an out-of-scope generation draining so it accepts no new streams, begin logical close, and release its
permit while accepted streams finish. Choosing a victim and balancing connection churn against new demand adds
a second fairness policy. Add it only if practical bounds and production-shaped multi-interface H2 traffic
reproduce the stall and provide evidence for that policy.

### HTTP/2 stream-credit pooling

The initial design keeps one accepting H2 generation per cell and does not pool peer
`SETTINGS_MAX_CONCURRENT_STREAMS` credit or open additional generations when that credit is exhausted. Hyper
continues to own stream-level readiness and flow control. The consequence is a possible throughput cliff
behind one generation rather than a correctness failure; configured partitions remain the explicit scaling
unit. Add stream-credit accounting or connection sets only after benchmarks demonstrate a material
stream-limit, flow-control, congestion-window, or throughput cliff and the Hyper integration can expose the
credit needed to make admission authoritative.

### Legacy builder shimming

The new pool and partitioned client can ship through additive APIs before they replace the existing
`Builder` and `ConnectorBuilder` internals. The later compatibility path should implement those legacy
builders as adapters onto this pool wherever their observable behavior can be represented faithfully, rather
than retaining a second connection-pool architecture behind deprecated entry points.

That shim requires a field-by-field audit of connector settings, idle defaults and nested-option semantics,
TCP and interface settings, proxy and TLS assembly, DNS overrides, runtime components, and custom connector
entry points. The implementation-neutral suites from smithy-rs PR #4767 are the acceptance baseline. A legacy
option that cannot be mapped exactly remains on the old path until an explicit compatibility decision is made;
the shim must not silently approximate it. Once the surface is covered, the hyper-util legacy pool can be
removed as an implementation dependency rather than kept alive solely by old builders.

---

## Appendix A: Public API and module structure

Appendix A assembles the callable construction surface. Types that define a mechanism remain with the
Architecture section that explains them: [partitions](#partitions), [origins](#origins-and-cells),
[reuse scope](#eligibility-and-capacity), [connection retirement](#why-a-connection-retires), and
[telemetry](#telemetry).

### Construction

A pool is built once and shared; clients are cheap handles onto one resolved partition.

```rust
#[derive(Clone)]
pub struct ConnectionPool {
    /* private: shared configuration and partition/origin state */
}

impl ConnectionPool {
    pub fn builder() -> Builder<TlsUnset>;
}

#[derive(Clone)]
pub struct Client {
    /* private: shared pool and resolved partition */
}

impl Client {
    pub fn new(pool: &ConnectionPool) -> Result<Self, ClientBuildError>;
    pub fn from_partition(
        pool: &ConnectionPool,
        id: PartitionId,
    ) -> Result<Self, ClientBuildError>;
}

#[derive(Debug)]
pub struct ClientBuildError { /* private */ }

impl ClientBuildError {
    pub fn partition(&self) -> Option<PartitionId>;
}
```

`Client` implements the smithy runtime's `HttpClient` through the
[Smithy client boundary](#smithy-client-boundary): each returned HTTP connector carries operation policy while
sharing this client's pool and resolved partition. `Client::new` resolves `PartitionId::ANONYMOUS`; it succeeds
only for a pool built without explicit partitions. `Client::from_partition` resolves the supplied identity,
including the anonymous identity when it exists. Either returns `ClientBuildError` rather than panicking when
the pool has no such partition. Resolution happens once at client construction, so a request performs no
partition lookup. `ClientBuildError` implements `Error`; `partition` returns the unresolved identity for the
current error kind without making that kind exhaustive.

[`ConnectionPool::stats`](#events-and-statistics) and the event API are specified with telemetry rather than
repeated here.

### Builder

TLS provider selection is the only typestate transition, and it gates only TLS configuration. Every other
setting is available in either state. Each setter has a `set_*` mirror taking `&mut self` and an `Option`, for
callers assembling configuration programmatically.

```rust
pub struct Builder<Tls = TlsUnset> {
    /* private: pool configuration and TLS typestate */
}

#[derive(Debug)]
pub struct BuildError { /* private */ }

impl<Tls> Builder<Tls> {
    pub fn idle_timeout(self, timeout: impl Into<Option<Duration>>) -> Self;
    pub fn set_idle_timeout(&mut self, timeout: Option<Option<Duration>>) -> &mut Self;
    pub fn time_source(self, source: impl TimeSource + 'static) -> Self;
    pub fn sleep_impl(self, sleep: impl AsyncSleep + 'static) -> Self;
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
    pub fn build_http(self) -> Result<ConnectionPool, BuildError>;

    // Test-only: gated behind `test-util` + `aws_sdk_unstable`. Injects a TCP-level
    // transport for tests; not general public surface, and does not honor interface binding.
    #[cfg(all(feature = "test-util", aws_sdk_unstable))]
    pub fn build_http_with_tcp_connector<C, IO>(
        self,
        connector: C,
    ) -> Result<ConnectionPool, BuildError>;
}

impl Builder<TlsProviderSelected> {
    pub fn tls_context(self, context: TlsContext) -> Self;
    pub fn build_https(self) -> Result<ConnectionPool, BuildError>;
}
```

Setters retain the supplied configuration and do not have eager and mutable forms with different validation
behavior. A terminal build validates the complete configuration and returns `BuildError` for a zero
`max_connections_per_host`, an explicitly supplied empty partition set, duplicate partition identifiers, or
an explicit partition using the reserved anonymous identity. `BuildError` reports the setting and value that
failed and implements `Error`; callers are not expected to branch on an exhaustive variant set.

When `partitions` is never set, construction creates the one anonymous, unbound partition. Once set, the
supplied nonempty set is the complete explicit topology and no anonymous partition is added.
`max_connections_per_host` is unset by default, and an unset bound constructs no admission machinery. When
set, it bounds one scheme-host-port origin across all partitions and interface groups, not per partition. HTTP
and HTTPS and distinct non-default ports are bounded separately. The limit counts every establishing, idle, and
active connection rather than only idle connections.

The initial builder exposes no pool-wide HTTP/1-only or HTTP/2-only policy. The connector determines the
negotiated protocol, while request version controls dispatch compatibility after negotiation and does not
configure connector ALPN. A future protocol-policy API requires a concrete Smithy caller and a definition of
how that policy composes when multiple clients share one pool.

An unset `idle_timeout` uses a 90-second default. Passing `None` to the fluent setter disables idle
timeout. The mutable setter preserves all three configuration states: outer `None` restores the default,
`Some(None)` disables the timeout, and `Some(Some(duration))` selects a duration. The builder's time source and
sleep implementation drive pool maintenance; their defaults are the production system clock and Tokio sleep.
They are pool inputs and are not replaced by per-operation `RuntimeComponents`.

Building retains configuration and assembles the reusable transport factory but opens no socket. Native trust
loading is deferred to the idempotent connector preflight invoked when the `Client` is selected by smithy, or
to first establishment when used without smithy validation. Whether a configured interface exists and can be
used is therefore reported later as a connector error on establishment, not `BuildError`. Likewise,
`TokioDriverSpawner::current` retains its own documented panic outside a Tokio runtime because that constructor
is invoked before the spawner is passed to the pool.

### Module structure

```text
aws-smithy-http-client/src/client/
  pool.rs              — ConnectionPool ownership and public re-exports
  pool/
    builder.rs         — Builder typestate, validation, connector assembly
    client.rs          — Client, PoolConnector, and ClientBuildError
    partition.rs       — partition declarations and runtime/interface placement
    origin.rs          — owned OriginKey, borrowed lookup, and canonicalization
    registry.rs        — PartitionRegistry, PartitionState, and stable cell publication
    cell.rs            — OriginCell and cell-level acquisition coordination
    cell/
      h1.rs            — HTTP/1 records, sender ownership, and reuse reservation
      waiters.rs       — local acquisition queue and delivery reservation
    admission.rs       — bounded-origin capacity and unlocked action driving
    admission/
      demand.rs        — versioned demand order and delivery fences
      reuse.rs         — H1 availability order and cross-cell reuse operations
      delivery.rs      — capacity/H1 crossing guards and acknowledgements
    establish.rs       — transport construction below protocol establishment
    establish/
      h1.rs            — HTTP/1 connect, handshake, installation, and driver
    dispatch.rs        — protocol-neutral request routing
    dispatch/
      h1.rs            — HTTP/1 acquisition, dispatch, retry, and response ownership
    maintenance.rs     — idle-deadline scheduling and partition task lifetime
    connection.rs      — records, leases, logical close, physical completion
    events.rs          — listener and lifecycle event types
    stats.rs           — origin/partition snapshots and lifecycle gauges
aws-smithy-http-client/src/
  sync/
    mod.rs              — standard-library and Loom backend selection
    std.rs              — production synchronization facade
    loom.rs             — modeled synchronization facade
```

The inventory describes ownership boundaries, not implementation order. The `pool` module re-exports every
public type above; private modules may be split or combined without changing the architecture so long as the
lock, ownership, and hot-path boundaries remain intact.

The transport-connector contract below the pool is unchanged: it is a `Service<Uri>` yielding
`(IO, Connected)`. The pool composes transport connectors and does not replace that contract; the smithy
`HttpConnector` above the pool is the request-policy facade described earlier.

---

## Appendix B: Validation

Validation supplies implementation evidence for the contracts above; it does not redefine them. The
implementation-neutral connection harness and existing-client behavior suites in smithy-rs
[PR #4767](https://github.com/smithy-lang/smithy-rs/pull/4767) form the compatibility baseline. Pool-specific
tests preserved on `archive/conn-pool-4708` are an additional inventory, not an acceptance target: applicable
contracts may be retained or rewritten, implementation-specific assumptions may be obsolete, and the owned
state machine requires coverage that prototype tests did not contain.

The evidence levels have distinct jobs:

* **Unit, property, and bounded transition tests** cover construction, canonicalization, indexing,
  accounting, and explicit state-machine transitions. Where focused state-space enumeration is used, the test
  identifies its operation alphabet and bound; ordinary transition tests are not described as exhaustive.
* Focused **Loom kernels** compile the production synchronization-bearing code against Loom and exercise
  concurrent cell publication, permit and H1 delivery, H1 selection and return, borrowed-H1 materialization,
  reuse cancellation, logical close, and maintenance publication or shutdown. They model these
  ownership boundaries rather than sockets or the complete network client. HTTP/2 generation publication and
  request-lease kernels are added with those mechanisms.
* **Controlled-runtime tests** use injected time, sleep, connectors, and executors to force cancellation at
  ownership-distinct cancellation boundaries, submitted-future drop, idle deadlines, independent-runtime
  request movement, explicit placement checks, and connector or handshake failure.
* The **wire harness** verifies HTTP/1.1 and HTTP/2 behavior against scripted peers, including reuse,
  multiplexing, ALPN, GOAWAY, stream reset, incomplete bodies, upgrades, poisoning, and transport close.
* **Differential tests** run the same implementation-neutral behavior contracts against the current
  Hyper-util-backed client and this pool. Any difference in request behavior, metadata, timeout scope, or error
  classification requires an explicit design decision rather than a rewritten oracle.
* **Benchmarks and stress tests** establish that the optimization and liveness contracts remain true at
  production concurrency and topology.

The required evidence maps to the architecture as follows:

| Mechanism                                                             | Primary evidence                                                                  | What it must establish                                                                                                                                                                                                                                         |
| --------------------------------------------------------------------- | --------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Construction, topology, origin identity, and stable cells             | unit/property; allocation instrumentation; controlled runtime; Loom cell creation | invalid configurations fail; equivalent URIs share one origin; canonical request hits allocate no host storage; each pair has one stable cell; the anonymous partition binds one runtime but moves across its workers; explicit placement follows its contract |
| Smithy `HttpClient` boundary and operation policy                     | unit; controlled runtime; differential                                            | settings-specific facades share one pool and admission authority; request version does not split pool or admission identity; timeout scope, maintenance ownership, validation timing, and `hyper/1.x` metadata are preserved                                   |
| Local reuse, establishment, ALPN convergence, and generation identity | unit/property; bounded transitions; controlled runtime; wire; differential        | local hits avoid origin-wide coordination; connector readiness and placement hold; one H2 flight/generation wins; losing transports, leases, and waiters terminate exactly once                                                                                |
| Admission, demand generations, and origin/group ordering              | property; bounded transitions; Loom scheduling kernels; stress                    | the bound is never exceeded; stale demand snapshots and availability reports cannot resurrect obsolete state; each resource uses the correct scheduling scope; eligible committed demand has bounded overtaking                                                |
| Capacity delivery, H1 reuse operations, and owning-cell turns         | bounded transitions; Loom delivery/reuse kernels; controlled cancellation         | every permit and provisional H1 has one owner; candidate transfer revalidates owning-cell state; acknowledgement fences close; cancellation and task drop refunnel once; return interception cannot starve owning-cell demand                                  |
| H2 publication and request leases                                     | bounded transitions; Loom publication kernel; wire                                | publication moves no capacity; generation gates prioritize committed waiters; stale generations cannot dispatch; send and receive endpoints both terminate before lease release                                                                                |
| Dispatch, retry, bodies, upgrades, and metadata                       | controlled runtime; wire; differential                                            | one selected sender commits and calls Hyper without an intermediate published state; only Hyper-certified unsent reuse retries; cancellation has a stage-local owner; H1 framing and H2 stream isolation hold; metadata and error behavior are preserved       |
| Logical and physical close, maintenance, events, and statistics       | unit/property; Loom close/guard/maintenance kernels; time/runtime; wire           | driver completion and cancellation request logical close; permit release occurs once; root-I/O drop ends physical accounting; idle deadlines and shutdown clean up; callbacks see committed state and gauges converge to lifecycle state                       |
| Locality, liveness, topology scaling, and retained memory             | bounded transitions; repeated stress; benchmarks                                  | grant work is independent of partition count; no reachable resource remains idle behind demand; local reuse does not regress; topology scales without moving I/O; physical-socket and route-memory costs are measured                                          |

Correctness acceptance requires every applicable unit, bounded-transition, Loom, controlled-runtime, wire,
and differential suite to pass. A bounded state-space enumeration reports neither an invalid state nor a
terminal accounting error and identifies the operation set and bound it actually explored; an ordinary
bounded-transition test makes no exhaustiveness claim. Focused liveness tests must show that usable
capacity or a compatible connection enables progress under the scheduling conditions they construct.
Concurrency-sensitive suites run repeatedly in CI; a flaky failure is a correctness failure, not benchmark
noise.

---

## Appendix C: FAQ

### Why not build the pool from composable connector layers?

Hyper's ecosystem offers pooling as connector middleware — a cache layer, a connection-limit layer, a
negotiate layer, each a `Service` wrapping the one below. The pool owns the coordination layer because the
state it coordinates is not local to any one layer, and stacked layers give no layer the whole picture.

Reuse and admission illustrate it. A connection limit as a middleware layer parks a request until a permit
frees, and a permit frees on logical close. Reuse is a different layer, and it wakes a waiter when a
connection returns to the idle set. Nothing connects the two: a request parked for a permit is not waiting on
the idle set, so an idle return does not wake it, and a request parked for reuse is not waiting on a permit,
so a close does not wake it. This already breaks in a single partition — a capacity-bound waiter is not woken
by the idle return that should satisfy it — and it is not a tuning bug in one layer but a consequence of the
lifecycle being split across layers that do not share a view of it. Coordinating capacity across partitions,
where a permit freed in one partition must wake a waiter another parked, is a further step the layered
stack has no structure to take at all; borrow and reclaim exist precisely because that path has to be a
first-class operation.

The [composable-pool prototype](https://github.com/smithy-lang/smithy-rs/pull/4708) had to vendor the cache
layer and carry SDK-specific modifications. Owning the cache, limit, and negotiate layers as one unit gives
them one lifecycle view. The pool therefore forgoes future upstream improvements to those layers, so its
equivalents must be as strong or stronger. The connector contract below the pool and Hyper's protocol
implementation above it remain unchanged.
