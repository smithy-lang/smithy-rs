# HTTP Connection Pool

## Requirements

### Existing HTTP client behavior is preserved

For equivalent configuration, the client preserves the existing client's observable behavior for HTTP
and HTTPS, HTTP/1.1 and HTTP/2, direct and proxied connections, DNS overrides, connect and read
timeouts, connection poisoning, and connection metadata capture.

This includes request-target form, proxy authentication, TLS negotiation, timeout scope, response-body
ownership, and error classification. Any intentional difference requires an explicit compatibility
decision rather than an implicit change in the pool.

### Connections to one origin are bounded

`max_connections_per_host = N` bounds admitted connections to one origin across every partition.
Connecting, handshaking, open active, and open idle connections count against the bound. Each origin has
an independent bound. The default is unbounded, and a configured value of zero is rejected.

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

The architecture begins with [the model](#the-model), which introduces topology, ownership, and one
end-to-end request path. [Topology and identity](#topology-and-identity) then defines partitions, origins,
cells, and their stable identities; the [Smithy client boundary](#smithy-client-boundary) explains how
operation policy reaches that shared pool.

The remaining sections follow a request through [local connection selection](#local-connection-selection),
[connection establishment](#connection-establishment), and
[bounded-capacity coordination](#bounded-capacity-coordination). [Liveness](#liveness) states the progress
guarantees and their limit. [Dispatch](#dispatching-and-completing-a-request) follows the selected connection
through request preparation, Hyper acceptance, response ownership, and cancellation.
[Connection retirement and maintenance](#connection-retirement-and-maintenance) covers return, maintenance,
and logical and physical close; [Telemetry](#telemetry) defines how those transitions are observed.

This order separates where state lives, how a request obtains a connection, and what retains or releases
that connection after dispatch. The model gives the complete path; each later section expands one segment.

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
current Tokio runtime on first establishment. The set is fixed at construction.

An **origin** is a scheme, host, and port — the web's origin as
[RFC 6454](https://www.rfc-editor.org/rfc/rfc6454) defines it, canonicalized so two spellings of one server
are one origin. The pool discovers origins lazily from requests rather than declaring them at construction.

A connection belongs to exactly one of each: one partition established it, and it can serve one origin. Their
intersection is an **`OriginCell`**, which holds the connections a partition has for an origin and is created
on first use of that pair.

```
                        origin: s3            origin: dynamodb
                      ┌──────────────────┐  ┌──────────────────┐
  partition 0         │   OriginCell     │  │   OriginCell     │
  runtime 0, eth0     │  idle H1 · H2 gen│  │  idle H1 · H2 gen│
                      └──────────────────┘  └──────────────────┘
  partition 1         │   OriginCell     │  │                  │
  runtime 1, eth1     │  idle H1 · H2 gen│  │  (never used)    │
                      └──────────────────┘  └──────────────────┘

  bounded only        OriginAdmission(s3)      OriginAdmission(dynamodb)
  spans partitions    admission · orders       admission · orders
                      peer-cell index           peer-cell index
```

Because the partition set is fixed and the origin set is not, partitions are the outer level: each partition
owns its own map from origin to cell, so the structure that grows is always inside one partition. A cell that
no request has asked for does not exist.

An **`OriginAdmission`** holds what all partitions sharing a bounded origin must agree on: its connection
budget, cross-cell demand order, and index of cells for that origin. Nothing else spans partitions.

#### Ownership and lifetime

```text
ConnectionPool                         config and fixed partition set
|-- Partition[]                        driver spawner, optional interface
|   `-- OriginCell by OriginKey        H1 records, H2 generation, local waiters
`-- OriginAdmission by OriginKey       permits, cross-cell orders, peer-cell index

Client ------------------------------> ConnectionPool + one resolved Partition
```

| Type              | Created                                | Destroyed                                     | Shared across partitions |
| ----------------- | -------------------------------------- | --------------------------------------------- | ------------------------ |
| `ConnectionPool`  | by the builder                         | when the last `Client` and request release it | —                        |
| `Partition`       | at construction, from the declared set | at pool drop                                  | no                       |
| `OriginCell`      | first request for (partition, origin)  | not while the origin is live                  | no                       |
| `OriginAdmission` | first request for a *bounded* origin   | not while the pool lives                      | yes                      |
| `Client`          | by the caller, freely                  | by the caller                                 | —                        |

`Client` is what a caller holds and what implements the smithy runtime's `HttpClient`. It pairs the pool with
one resolved partition, so a request never searches for its partition — the handle already names it.

An `OriginAdmission` exists only for a bounded origin. It has work to do only when a bound can force borrow or
reclaim: on a local miss a partition establishes its own connection, and it borrows a peer's dispatch handle
only when it *cannot* establish, capacity being bounded and no permit free. An unbounded origin never borrows
or reclaims, so it has no origin-wide admission or cross-partition structure; its cells are the whole of it.
Reuse scope governs which cells may relieve one another's admission pressure, so it has no effect until a
bound makes that pressure possible.

The tree above shows where state is stored. Pool retention and post-header protocol ownership are distinct from
bounded-origin capacity ownership:

```text
pool lifetime

caller-held Client ------------------------------> ConnectionPool
request future, until response head/error -------> ConnectionPool
```

These two references keep the whole pool alive. Producing a response head or terminal error ends the request
future's pool hold. The response transfers the remaining protocol lifecycle to different owners:

```text
post-header protocol lifetime

H1 checked-out connection <--- response body or upgrade-bridge lifecycle guard

H2 request lease
  |-- receive endpoint <------ response body or upgrade bridge
  `-- send endpoint <--------- accepted H2 request-body adapter
```

At its terminal protocol boundary, the H1 guard either returns a reusable connection or retires it. The H2
lease is released only after both stream endpoints terminate.

A bounded origin's connection permit has a separate exactly-one-owner path:

```text
bounded connection capacity

OriginAdmission
  `-- issue --> establishing task owns CapacityLease
                    +-- failure or drop --------> return lease to OriginAdmission
                    `-- install connection -----> connection record owns CapacityLease
                                                     `-- logical close -> return lease to OriginAdmission
```

At each arrow, the source relinquishes the lease before the destination owns it. Dispatch handles and H2
request leases never own a connection permit.

#### Request path

One request takes the same path until local connection selection misses. Only then do unbounded and bounded
origins differ:

```text
request on Client(partition P, URI)
  |
  +-- canonicalize URI -> borrowed origin lookup O
  `-- resolve or create P.origins[O] -> OriginCell (own O only on insertion)
          |
          +-- usable local H1 connection or H2 generation? -- yes --> dispatch
          |
          `-- no
              |
              +-- establishment allowed? -- yes --> acquire lease when bounded
              |                                  --> connect + TLS/ALPN + Hyper handshake
              |                                  --> record or publish connection
              |                                  +-- compatible --> dispatch
              |                                  `-- incompatible --> keep for compatible demand;
              |                                                       return compatibility error
              |
              `-- no: bounded origin has no free permit
                  |
                  `-- park behind the cell's demand ticket
                      |
                      +-- eligible reusable connection becomes available --> dispatch
                      |
                      `-- released or reclaimed permit
                          `-> establish on P --> dispatch

dispatch
  |
  +-- terminal error -------------------------------> release or retire owned state
  |
  `-- response head --> response body or upgrade reaches a terminal outcome
                           |
                           +-- reusable H1 ----------------> return to source cell
                           +-- H2 request ends -------------> release request lease; generation may stay
                           +-- protocol upgrades -----------> transfer lifecycle ownership
                           `-- connection retired ----------> logical then physical close
```

[Local connection selection](#local-connection-selection) defines local selection.
[Connection establishment](#connection-establishment) defines establishment, placement, and protocol
convergence. [Bounded-capacity coordination](#bounded-capacity-coordination) defines parking, borrow, reclaim,
and resource delivery, with [Liveness](#liveness) stating when those paths guarantee progress.
[Dispatching and completing a request](#dispatching-and-completing-a-request) defines request preparation,
stale-reuse retry, and the transfer to response or upgrade ownership. The path ends in
[return or retirement](#connection-retirement-and-maintenance), where each terminal outcome either makes the
connection reusable or closes it. This diagram is a map of those sections, not a second specification of
their transitions.

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

The interface binding is applied to the socket before connect — `SO_BINDTODEVICE` on Linux-like systems,
`IP_BOUND_IF` on macOS-like and Solaris-like ones — so a connected socket's egress interface is fixed for its
lifetime. That immutability is what makes handing a dispatch handle to another partition safe: it cannot move
bytes off the interface the caller chose.

A pool with no declared partitions has exactly one, unbound, which is the right shape for a program that has
not reasoned about placement and uses one Tokio runtime. Its first establishment binds that anonymous
partition to the current runtime; later requests may run on any worker thread of the same runtime. Partitions
without an interface compare as one group, so the common case does no per-request interface work. The
anonymous partition has the reserved identity `PartitionId::ANONYMOUS`, used by events and statistics;
callers cannot declare it explicitly. Every explicit identifier is caller-owned. A thread-per-core caller can
therefore reconstruct `PartitionId::from_index(thread_id)` when it declares the topology, creates each
thread's client, and reads per-partition statistics, without plumbing pool-issued handles between those sites.

`TokioDriverSpawner::current` captures the current Tokio handle eagerly and panics when called outside a
runtime. `from_handle` takes a specific handle. Both spawn on the captured runtime regardless of which thread
invokes `spawn`; neither is supplied by the caller for the anonymous partition, which captures its runtime on
first establishment as [Connection establishment](#connection-establishment) describes.

`Partition::interface` is available only on platforms where the binding can be applied. The interface is a
construction-time value but its existence and permissions are properties of the host when a socket is opened;
those failures are connector errors rather than pool-construction errors.

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
Host comparison is already ASCII-case-insensitive. Two spellings are deliberately *not* unified: an
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

The implementation reads explicit port text from the already-validated `Authority` rather than relying only
on `Authority::port_u16()`. That accessor returns `None` for both an absent port and text that cannot be
represented as a nonzero `u16`; preserving the distinction prevents malformed, zero, or out-of-range ports
from aliasing the scheme's default-port origin.

The request's HTTP version is deliberately absent. A request marked HTTP/1.1 may dispatch on an HTTP/2
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
connections are gone. The initial design deliberately accepts that retention to preserve stable identity; see
[Reclaiming quiescent origins](#reclaiming-quiescent-origins).

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
  its first establishment binds one runtime, and every later establishment and driver uses that same runtime
  while requests may move among its worker threads.
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

```
request on partition P for origin O
  P is resolved on the client handle              no lookup
  → P.origins[O]                                  partition-local
  → cell: take a live idle connection             cell-local
```

That is the entire path for a reuse hit. It performs no origin-wide coordination, reads no other partition's
state, and touches no `OriginAdmission` or peer index — its synchronization is the requesting
partition's own cell lock. A peer can acquire that same lock when the origin is bounded and under pressure, to
borrow a handle or claim a connection as it returns, so the lock can be contended; what the hit never does is
consult state shared across the origin. So a pool with one partition and a pool with ninety-six do the same
work per uncontended hit, and a request for one origin is unaffected by traffic to another. This
is the payoff of partitions being the outer level: the map that grows with the origin count lives inside a
partition, off every other partition's path.

A reused connection may be dead: the server can close an idle connection while it sits in the cell, and the
pool learns this only on dispatch. So "take a live idle connection" is provisional until the request is
accepted. Hyper's `try_send_request` returns the unsent request when the connection failed before accepting
it, and that returned request is the retry boundary — a reused connection that fails before acceptance is
transparently retried on a fresh one, invisibly to the caller. A request the connection had already accepted
is not retried here; whether to retry it is the caller's policy, because the pool cannot know the request was
not acted on. This distinguishes a *reused* connection, where a pre-acceptance failure is the expected
stale-idle race and is absorbed, from a *fresh* one, whose failure is a real error the caller sees.

On a local miss, one acquisition episode may wait for a compatible H1 return while it prepares establishment.
The returned H1 and the establishment result compete for that episode, and exactly one result is committed to
the launching waiter. If the H1 wins before the connector is first polled, connector work remains lazy and any
tentative capacity lease is returned. The first connector poll is the ownership boundary: after it, the
establishment authority belongs to the pool rather than the launching request and continues even when a
returned H1 serves that request. A successful losing H1 attempt is installed for successor demand or idle
reuse; a result that negotiates H2 follows the ordinary flight and publication path. Failure releases the
attempt's resources and drives normal pool progress and telemetry, but cannot change the result already
delivered to the launching waiter.

If establishment wins, a concurrent H1 return follows ordinary source return handling. If no capacity is
available, no establishment attempt starts and the request follows the bounded waiting and return-claim path.
Cancellation removes the launching waiter but does not cancel an establishment authority that has crossed the
first-poll boundary; every returned connection, attempt, lease, and waiter still has one terminal owner.

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

Which runtime that is depends on the partition. The **anonymous partition** has no caller-supplied runtime. Its
first establishment captures the current Tokio runtime, and every later establishment and its driver run on
that same runtime. The default `Client` — the shape every generated smithy-rs client uses — may therefore
travel freely across worker threads of one multithreaded runtime, which share its I/O driver, but not across
independent runtime instances. An **explicit partition** names a specific runtime through its
`DriverSpawner`, and its client must be driven from that runtime; a thread-per-core service that pins a
runtime per core holds each partition's client on its core, so the precondition costs nothing. Driving either
kind of partition's client from a different runtime contradicts its placement.

`TokioDriverSpawner` always submits the driver to its captured handle and debug-asserts that the current
runtime's stable `Handle::id()` matches the captured handle. The assertion turns foreign-runtime use into a
test or debug-build failure; it is a diagnostic rather than enforcement because the socket is created before
the driver is spawned. Tokio may reuse an ID after its runtime is dropped, so the check is not a persistent
runtime identity or a substitute for the placement ownership rule.

Hyper spawns work of its own, and it follows the connection. An HTTP/2 connection hands Hyper a connection
task at handshake and per-stream and upgrade tasks as it runs, through an
[`Executor`](https://github.com/hyperium/hyper/blob/v1.11.0/src/rt/mod.rs#L45) the caller
supplies; the pool supplies one that forwards to the connection's runtime. HTTP/1 spawns nothing and needs no
adapter.

Enforcing placement for an explicit partition driven from a foreign runtime — rather than requiring the
caller to honor it — would mean running establishment as a task on the partition's spawner and delivering its
result back to the requester, at the cost of a spawn and a wake on every establishment. That path is
[future work](#future-work).

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
ALPN, the owner performs one cell-local claim-or-join transition before starting the Hyper protocol handshake:

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

A `DemandId` names one episode for the cell's current queue head and may receive at most one terminal
acquisition outcome. Its protocol requirement is stable. Serving or cancelling that head terminates the
demand; if useful demand remains, the cell creates a successor ID for the new head. `SnapshotVersion` orders
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
                    +-- reusable H1 appears
                    |     `-- merge the group-compatible and origin-capacity heads
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
  +-- R or its reserved waiter is stale -> return H1 to source
  `-- still compatible -----------------> move checked-out H1 guard to waiter

terminal outcome for R
  +-- no useful demand remains -> ticket becomes idle
  `-- useful demand remains ---> publish successor revision at applicable tails
```

The final local probe is part of accepting capacity, under the cell lock. It prevents a permit delivered
concurrently with local return or H2 publication from causing an unnecessary new connection. Local progress,
waiter cancellation, and host delivery therefore race through revision validation rather than by trying to
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

Because eligibility and capacity are separate, capacity reaches a waiting cell in one of two ways, depending
on which question is blocking.

**Borrow** lends a dispatch handle: a cell holding a reusable connection (the *source*) grants dispatch
access to the cell that wants one (the *target*), and the connection stays open on the source's partition.
The target sends requests through the handle, but the bytes still run on the source's runtime and interface,
so borrow is confined to what the reuse scope permits — a handle is lent only within an eligibility group.
It answers the case where a warm, eligible connection exists in another cell.

**Reclaim** transfers a permit: a reusable H1 connection is closed and its freed permit lets the waiting cell
establish. The connection may be in that cell or another one. Nothing dispatches across a partition boundary,
so cross-cell reclaim is not constrained by the reuse scope. It answers the case where the origin is at its
limit and the waiter needs its own connection. Which connection is closed is a retirement decision, covered in
[Connection retirement and maintenance](#connection-retirement-and-maintenance).

The bounded origin's peer-cell index supplies sources without searching cells or connections. A cell with at
least one open H1 record has one source advertisement in an origin-wide view and its eligibility-group view.
The first H1 record adds it, and logical close of the last removes it; checkout and return do not change
membership, so ordinary H1 reuse remains cell-local. A source with a nonterminal claim, or with a currently
usable owed local turn, is temporarily unavailable for another cross-cell claim.

A demand-driven claim turn takes the origin head. When that head accepts H1, admission first takes a source
from its group view so the warm connection can be borrowed; otherwise it takes an origin-wide source and
reclaims. A return-driven turn already names its source and performs the target merge below. Taking a source
rotates its advertisement until its claim attempt finishes. The source lock then validates whether an H1 is
idle, active and able to return, or gone; rejection updates a stale advertisement and advances another bounded
turn. The advertisement is a scheduling hint, not a connection handle or capacity owner, and exact list or
index representation remains private.

Cross-cell borrow and reclaim use a bounded *return claim*. Admission owns the claim record; the source cell
owns one claim slot that can intercept at most one H1 return; the target cell owns its local waiter and demand
revision. Same-cell H1 service and reclaim do not need a cross-cell claim: when the scheduling merge selects
the source's own compatible head, the H1 stays local; when it selects the source's H2-required capacity head,
the H1 closes and its permit follows the ordinary capacity-delivery path. The logical claim states are:

```rust
enum ClaimMode {
    BorrowOnly,
    ReclaimOnly,
}

enum ClaimPhase {
    Installing,
    Installed,
    Resolving,
    Cancelling,
}

enum ClaimEndpoint {
    Pending,
    Complete,
}

struct ReturnClaim {
    id: ClaimId,
    source: CellId,
    target: CellId,
    demand: DemandId,
    mode: ClaimMode,
    phase: ClaimPhase,
    source_endpoint: ClaimEndpoint,
    target_endpoint: ClaimEndpoint,
}

enum SourceClaimState {
    Available,
    Installed(ClaimId),
    Resolving(ClaimId),
}

struct SourceClaimSlot {
    claim: SourceClaimState,
    local_turn_owed: bool,
}
```

These are protocol states, not a required storage layout. `BorrowOnly` means the distinct source and target
are in one eligibility group and the published head can accept H1; the target revalidates both facts.
`ReclaimOnly` means no dispatch handle may cross the boundary, so a candidate can move only through logical
close into a permit. It does not convert from one mode to the other after installation: doing so would reuse a
group-scoped target for an origin-scoped grant. Each source has at most one nonterminal claim, and each claim
names one target demand revision.

A provisional H1 candidate names its source generation and return claim. Each source-cell transition that
extracts or confirms it revalidates the same facts as an ordinary H1 return: the named generation is still
installed and dispatch-eligible, it is not poisoned or logically closing, idle policy still permits use, and
the source claim still names that candidate. Failure rejects the claim and returns the H1 through ordinary
source handling. Source and target locks remain unnested; a close that wins after source revalidation is the
ordinary stale-selection race and is caught again before dispatch.

Claim installation and resolution follow one path. A borrowed candidate always returns through admission
before it reaches a target; the source and target never hand the H1 directly between their cell locks:

```text
OriginAdmission owns queued target revision R and ReturnClaim K
  |
  `-- install K on source under source-cell lock
        |
        +-- a turn is owed to the current H1-compatible local head,
        |   or an older compatible local head is waiting
        |     `-- reject K; preserve owed turn or older local priority; target R stays queued
        |
        +-- idle H1 available
        |     `-- source -> Resolving(K); candidate guard takes H1
        |
        +-- active H1 exists
        |     `-- source -> Installed(K); next reusable return is reserved
        |
        `-- no H1 can return
              `-- reject K; target R stays queued

Installed(K) + H1 response completes
  `-- under source lock: Installed(K) -> Resolving(K); candidate guard takes H1

candidate reaches OriginAdmission after source-cell revalidation
  |
  +-- claim or R is stale, cancelled, superseded, or already satisfied
  |     `-- reject; candidate guard returns H1 through source's ordinary return path
  |
  +-- BorrowOnly remains valid
  |     `-- PeerPending(R, K) -> Delivering(R, D, BorrowedH1)
  |           `-- DeliveryGuard<ProvisionalH1> crosses to target
  |                 +-- reserve R's oldest compatible waiter; commit H1 guard
  |                 `-- reject; guard returns H1 to source before acknowledging D
  |
  +-- BorrowOnly is no longer valid
  |     `-- reject; return H1 to source; admission reruns target selection
  |
  `-- ReclaimOnly and R still needs capacity
        `-- guarded reclaim action returns to source
              `-- revalidate and logically close source H1
                    `-- released permit tagged K enters capacity delivery for R

source completion + target completion
  `-- admission removes K only after both endpoints acknowledge terminal state
```

For `ReclaimOnly`, the action commits a capacity delivery for `K` only when its logical-close transition
releases the candidate's capacity lease. If another close wins after revalidation, the claim is rejected and
the still-live demand revision remains in ordinary admission order; the losing reclaim neither tags nor
duplicates the permit released by the winning close.

An installed claim wins the return race under the source lock. Compatible local demand arriving after
installation may therefore be overtaken once; an irreversible cross-cell borrow or reclaim then changes the
source's `local_turn_owed` bit when compatible local demand exists. When that demand is the current head and
can accept H1, the source is not externally claimable; the next H1 return or other service of that demand
clears the turn. If compatible local demand drains first, the turn clears without consuming a connection and
the source is advertised again.

An H2-required local head is not bypassed to manufacture the turn. The owed bit remains set, but while that
head is current it does not block a cross-cell claim or a same-cell reclaim that can make progress. When an
H1-compatible head reaches the front, the source enforces the owed turn. Keeping claim occupancy and the turn
as separate state prevents an unusable local H1 from stranding both the local H2 head and an external target.

The turn is earned when the source transfer becomes irreversible: a borrow commits the provisional H1 to the
target, or a reclaim logically closes the source H1. Rejection or cancellation before that point creates no
turn. Cancellation after it does not revoke the turn, even when an H1 returns immediately or a released permit
is refunnelled.

The terminal target acknowledgement records whether that point was crossed. Admission then completes the
source endpoint only after a guarded source-cell action sets `local_turn_owed` when compatible demand exists.
The claim remains authoritative until that action acknowledges, so task drop cannot lose the fairness debt.

Cancelling a target while installation is in flight marks the claim cancelling. The source processes install
before cancel, so cancellation cannot overtake an action that may already have reserved a return. If
cancellation wins while
the source is `Installed`, the slot's claim becomes `Available`. If the source is already `Resolving`, the
guarded H1 returns through ordinary source handling. Local demand can consume it there before it becomes idle.
Cancellation does not clear a previously owed local turn.

Every cross-lock claim action has a typed fallback. Dropping an uncommitted install reports rejection and
clears the source slot's claim. Dropping a provisional H1 action returns that H1 to the source. Dropping a
reclaim capacity delivery refunnels the permit through admission. A claim record remains authoritative until
source and target endpoint acknowledgements arrive, so a task drop cannot make the same source claimable twice
or abandon a target reservation. Fallback actions run no connector, protocol, wake, or listener code while a
pool lock is held.

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
error: a partition with no permit of its own borrows a
peer's handle or is handed capacity by reclaim, and makes progress without a connection of its own. The
default `NetworkInterface` scope uses both; `Partition` and `Pool` are the same machinery with a narrower or
wider eligibility group.

#### Ordering across cells

Within a cell, its queue orders requests. Across cells competing for one origin, admission orders demand
episodes at two scopes because resources do not all reach the same targets:

```text
OriginAdmission(O)
  origin capacity order (all heads):
    oldest -> C2/R8(H1) -> C0/R3(H2) -> C3/R5(H2) -> C1/R9(H1)

  eligibility group eth0, all-protocol view:  C0/R3(H2) -> C1/R9(H1)
  eligibility group eth0, H1-compatible view: C1/R9(H1)
  eligibility group eth1, all-protocol view:  C2/R8(H1) -> C3/R5(H2)
  eligibility group eth1, H1-compatible view: C2/R8(H1)

  permit or reclaim: take origin capacity head
  peer H2:           take source group's all-protocol head
  borrowed H1:       take source group's H1-compatible head
```

An active demand revision has one common insertion sequence. It is linked in the origin-wide capacity order,
its eligibility group's all-protocol view, and, when its stable head accepts H1, that group's H1-compatible
view. The second group view is necessary: using only the all-protocol head would either strand an H1 behind an
H2-required waiter that cannot use it or require an unbounded search. A **borrowable H1 handle** can serve only
the compatible group view; peer H2 publication can serve the all-protocol group view; a **permit** freed by
logical close or moved by reclaim can serve any cell in the origin. The scopes coincide when there is one
eligibility group, including the common case where all partitions are unbound.

A reusable H1 is both a group-scoped handle and a possible origin-scoped reclaim opportunity. Claim placement
merges those choices without letting a hot group consume every return ahead of older capacity demand:

```text
choose action for reusable H1 from source S
  |
  `-- read two O(1) heads using their common episode sequence
        B = oldest H1-compatible demand in S's eligibility group
        C = oldest capacity demand in the origin
          |
          `-- select B when B exists and is no younger than C; otherwise select C
                |
                +-- selected B names S -> serve S's local compatible head; no claim
                +-- selected C names S -> close H1; deliver permit to S; no cross-cell claim
                +-- owed turn is currently usable -> preserve local turn; no cross-cell claim
                +-- selected B --------> install BorrowOnly(B)
                +-- selected C --------> install ReclaimOnly(C)
                `-- neither exists ----> no action
```

The common sequence ensures that a younger group-local borrower cannot take the H1 when an older cell in
another group is waiting for reclaimable capacity. If the oldest origin demand is also eligible and
H1-compatible, the two heads name that same cell and borrow preserves the warm connection. A source-local
H2-required head can instead reclaim its own H1 without manufacturing a cross-cell claim. The owed-turn bit
deliberately permits one source-local overtake after an irreversible cross-cell transfer; after that turn, the merge
applies again. Peer H2 publication is non-destructive — the source record keeps its permit and generation — so
it follows only the all-protocol eligibility-group view and does not consume an origin-wide reclaim opportunity.

Each grant or target choice is a dequeue or stored-head comparison, not a search. Grant work is therefore
constant independent of the number of cells or partitions. A terminal outcome removes the revision from all
of its views; a successor gets a new sequence and joins the applicable tails, which prevents a continuously
busy cell from retaining its old position.

#### Delivery

A released permit or provisional H1 can serve only one waiter. It must cross from admission to a cell without
being lost, copied, or left attached to a cancelled demand episode. A published H2 generation is different:
the connection record retains its permit and many compatible requests may take request leases from it. The
logical states keep these two cases separate:

```rust
enum DeliveryState {
    Idle,
    Queued {
        demand: DemandSnapshot,
    },
    PeerPending {
        demand: DemandSnapshot,
        claim: ClaimId,
    },
    Delivering {
        demand: DemandSnapshot,
        delivery: DeliveryId,
        kind: DeliveryKind,
    },
}

enum DeliveryKind {
    Capacity,
    BorrowedH1,
    PeerH2Publication,
}

enum AcquisitionPayload {
    Capacity(CapacityLease),
    BorrowedH1(ProvisionalH1),
}

enum TargetAckResult {
    Accepted { successor: Option<DemandSnapshot> },
    RetrySameResidence,
    Rejected { successor: Option<DemandSnapshot> },
}

struct DeliveryGuard {
    delivery: DeliveryId,
    target: CellId,
    demand: DemandId,
    state: DeliveryGuardState,
}

enum DeliveryGuardState {
    Undelivered {
        payload: AcquisitionPayload,
        on_drop: TargetAckResult,
    },
    Committed(TargetAckResult),
    Disarmed,
}

struct PublicationGuard {
    delivery: DeliveryId,
    target: CellId,
    demand: DemandId,
    source: CellId,
    generation: GenerationId,
    state: PublicationGuardState,
}

enum PublicationGuardState {
    Pending {
        on_drop: TargetAckResult,
    },
    Committed(TargetAckResult),
    Disarmed,
}
```

These are logical ownership states. `Queued` is linked in the origin order and its applicable group views.
`PeerPending` retains that residence while one return claim is in flight and remains eligible for a direct
permit or H2 publication; any of those may win and cancel the claim. `Delivering` is a fence at the ticket's
current order position. It names one delivery and remains until the target acknowledges what happened, so a
younger ticket cannot pass while the target is between locks.

An owned one-to-one delivery follows this sequence:

```text
OriginAdmission lock
  Queued/PeerPending(R)
    -> Delivering(R, D, kind)
    -> extract DeliveryGuard::Undelivered { payload, on_drop: retry R }
unlock OriginAdmission
  |
  `-- lock target cell
        +-- R and target waiter are still live and compatible
        |     -> move payload into authoritative target state
        |     -> guard = Committed(ack with complete successor state)
        |
        `-- stale, cancelled, already satisfied, or incompatible
              -> guard retains payload and records RetrySameResidence or Rejected
      unlock target cell
          |
          `-- finish guard: Committed submits ack; Undelivered refunnels then submits on_drop
                `-- lock OriginAdmission
                      -> apply ack and complete demand snapshot
                      -> close D's fence
                      +-- retry R at same position, or
                      +-- retire R, or
                      `-- enqueue successor at applicable tails
```

The admission lock publishes `Queued`, `PeerPending`, and `Delivering`; the cell lock publishes waiter,
compatibility, and local payload state. The locks are never nested. Between them, the delivery guard is the
only owner of a permit or provisional H1. Committing capacity moves its lease to an establishing task;
committing an H1 moves its checked-out guard to one waiter. If cancellation occurs after local commit but
before the waiter consumes the payload, the target's authoritative state owns it and cancellation extracts and
refunnels it before acknowledging the delivery.

The guard makes every drop point terminal. Dropping `Undelivered` returns a capacity lease to admission or an
H1 to its source's ordinary return path, then rejects or retries the matching fence. Dropping `Committed`
submits the stored acknowledgement; it cannot recover a payload already moved into target state. Normal
execution submits the acknowledgement and disarms the guard. Delivery identities and demand revisions make
repeated or delayed acknowledgement stale rather than destructive.

`Accepted` consumes the revision and either idles the ticket or installs its successor at the applicable tails.
`RetrySameResidence` is used only when the same revision remains useful but this source or publication cannot
serve it; it preserves the ticket's position. `Rejected` closes the old residence after the target has already
refunnelled any owned payload and carries the complete current successor, if one. A complete newer demand
snapshot may retire or replace a residence before its action reaches the target; local revision validation
then rejects the late action without resurrecting old demand.

#### HTTP/2 publication

H2 publication carries no `AcquisitionPayload`. The source connection record continues to own the capacity
lease, while a `(source cell, generation identity)` notice says that compatible requests may attempt to take
H2 request leases. Publishing a new local generation first installs the record and accepting generation under
the source cell lock, then makes that identity visible to compatible local waiters. They are woken and admitted
in bounded local turns; publication does not scan or synchronously wake an unbounded queue.

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
advertisement or new group demand schedules a bounded publication turn from stored source and target heads.
The advertisement carries only source and generation identity, not a dispatch handle or capacity lease, and
the publication guard revalidates it at the source and target. A stale advertisement is removed or updated
before the next turn, so peer H2 discovery does not scan cells.

Under bounded pressure, an accepting generation may also be announced to the head of its eligibility-group
all-protocol view. The ticket enters `Delivering` with `PeerH2Publication`, but the action carries only the
generation identity and a publication guard with acknowledgement fallback — never the record's capacity lease.
The target
revalidates the generation identity, accepting state, demand revision, and reuse scope. Acceptance makes the
generation visible to compatible waiters in that target cell; rejection discards the stale notice while the
generation and permit remain at the source. Acceptance acknowledges after target-local visibility and the
named head's activation opportunity are committed, not after every local waiter has activated. Remaining local
waiters proceed through the generation gate in bounded turns and no longer advertise a connection need while
that generation remains usable. Later group tickets are handled by subsequent bounded publication turns, so
one-to-many visibility does not turn one host action into work proportional to partition count.
Dropping a pending publication guard submits its `on_drop` acknowledgement so the fence retries or closes;
there is no single-owner payload to refunnel. Committing publication stores the target acknowledgement, which
is submitted before the guard disarms.

This separates publication from single delivery: a permit or H1 has one owner and one target, while an H2
generation remains source-owned and may be announced repeatedly. The transitions must be model checked rather
than accepted by inspection; their invariants are stated in [Correctness invariants](#correctness-invariants),
with the checks specified in [Appendix B](#appendix-b-validation).

#### Obligations

* **Bounded demand** [safety] — a cell carries at most one active demand revision regardless of how many
  requests wait, so demand accumulates no deficit and one residence receives at most one terminal outcome.
* **Snapshot ordering** [safety] — admission retains the newest complete demand version and rejects an action
  for a retired revision, so out-of-order publication cannot resurrect cancelled or satisfied demand.
* **Eligibility and capacity independence** [safety] — the capacity decision and the eligibility decision do
  not read each other's state.
* **Placement under transfer** [safety] — neither borrow nor reclaim moves a connection's driver or I/O off
  its owning partition.
* **Reclaim scope independence** [safety] — reclaim moves a permit without dispatching across a partition
  boundary, so it is not constrained by the reuse scope.
* **Single delivery** [safety] — one delivery identity owns at most one permit or provisional H1, commits it
  to at most one target waiter, and retains its scheduling fence until target acknowledgement.
* **Refunnelling** [safety] — rejection, supersession, cancellation, task drop, or panic returns every
  undelivered permit to admission and every undelivered H1 to its source exactly once.
* **Publication ownership** [safety] — H2 publication carries generation identity, never the connection's
  capacity lease; request activation takes a request lease while the source record remains the capacity owner.
* **Publication priority** [liveness] — publication closes the generation gate before visibility and offers
  activation to waiters committed at publication before newer arrivals, in bounded oldest-first turns.
* **Cross-cell order** [safety] — peer H2 takes the all-protocol group head, borrowed H1 takes the H1-compatible
  group head, and permits and reclaim take the origin-wide head; a returning H1 compares its compatible-group
  and origin heads by their common episode sequence.
* **Claim endpoint completion** [safety] — a return claim remains authoritative until source and target
  endpoints acknowledge terminal state and any earned local turn is recorded; no source has more than one
  nonterminal claim.
* **Return interception** [liveness] — an installed claim intercepts the next reusable H1 before it becomes
  idle, so a source cycling continuously between active and reusable cannot strand its target.
* **Source fairness turn** [liveness] — one irreversible cross-cell transfer creates one source-local turn when
  compatible local demand exists; the turn clears only when that demand is served or disappears.
* **Acknowledged progress** [liveness] — every extracted delivery or claim action either acknowledges a
  terminal transition or executes its typed fallback, so a scheduling fence cannot remain pending solely
  because the executing future was dropped.
* **Cross-lock isolation** [safety] — admission and cell locks are never nested, and no pool lock is held
  across an await or while running connector, protocol, wake, or listener code. A synchronous fallback may
  visit a bounded sequence of lock domains but holds at most one pool lock at a time; each transition retains
  an idempotent fallback, and wakes and callbacks remain deferred until after unlock.
* **Bounded peer discovery** [optimization] — claim and publication work select source state from stored origin
  or group heads, validate one cell, and rotate stale state rather than scanning cells or connections; active
  and idle H1 transitions do not update the shared source index.

### Liveness

Three guarantees keep a committed waiter moving, and together they are the mechanism behind
[Eligible requests make progress](#eligible-requests-make-progress).

Progress is eventual for a committed waiter, and the ordering that makes it so is the two scopes from
[Bounded-capacity coordination](#bounded-capacity-coordination): a cell's queue orders its own requests; origin
admission orders cells for capacity; and each eligibility group orders cells for the connections they may
reuse. Permits and reclaim take the origin head, while borrowed H1 and peer H2 take the compatible group view.
The common episode sequence resolves a returning H1 that could either be borrowed or reclaimed, and a terminal
outcome sends any successor revision to the applicable tails. A source fairness turn permits one deliberate
local overtake after an irreversible cross-cell transfer, but repeated claims cannot keep the source or an
older peer
from progressing.

Within a cell, the generation gate offers a newly published H2 generation to already committed compatible
waiters before newer arrivals. Scheduling is work-conserving among eligible waiters: if a resource a waiter
could use is free, some eligible waiter is served rather than the resource sitting idle. Each choice is a
dequeue or stored-head comparison, so the work to grant one resource does not grow with the number of waiters
or partitions.

There is one condition under which a waiter does not progress, and it is a deliberate limit rather than a
defect. Progress requires that a permit become reachable — an eligible connection returns reusable, an
HTTP/1 connection becomes reclaimable, or a permit is released. It is not promised while every permit for the
origin is held indefinitely by active HTTP/2 work that the waiter is not eligible to use. The pool does not
forcibly drain a live HTTP/2 connection to free such a permit; doing so would abort in-flight requests to
serve a waiter, trading one starvation for another. A waiter in this state parks until eligibility or
capacity changes on its own. This bounds what the no-starvation guarantee covers, and the boundary is
visible to operators through the pool's statistics rather than hidden.

#### Obligations

* **Bounded overtaking** [liveness] — a committed cell is not passed indefinitely by later arrivals; permits
  and reclaim use the origin-wide order, peer H2 uses the all-protocol group view, borrowed H1 uses the
  H1-compatible group view, and a returning H1 is assigned by the common episode sequence when those scopes
  compete.
* **Work-conserving service** [liveness] — while an eligible waiter and a resource it may use both exist,
  some eligible waiter is served.
* **Bounded grant work** [optimization] — the work to grant one resource does not grow with the waiter or
  partition count.

### Dispatching and completing a request

Acquisition ends with one request and one selected dispatch authority. Dispatch turns those into either a
terminal error or a response whose body, or upgrade path, owns the request's remaining protocol lifecycle.
This section defines that ownership transfer. It deliberately leaves HTTP framing, stream state, and
flow-control behavior to Hyper.

#### Preparing the request

The request future retains the original absolute URI while the request is in the pool. The origin key and
proxy decision are made from that URI before any request-target rewrite. Existing request validation and proxy
authentication behavior is preserved, including rejection of unsupported HTTP versions and HTTP/1.0
`CONNECT`, and insertion of proxy authorization only for an applicable cleartext HTTP proxy when the caller
did not supply it.

Protocol compatibility is checked against the selected connection before the request is moved into Hyper. An
HTTP/2-marked request cannot use H1; an HTTP/1.1-marked request may use H2. An incompatible H1 selection is
returned to its source if it remains usable, and the request receives the existing unsupported-version error.
This applies to a fresh automatic-ALPN attempt that resolves to H1: the pool keeps the H1 for compatible
demand and does not establish repeatedly in hope of negotiating H2. The compatibility error and connection
capture both identify the H1 connection that was selected.

For H1, dispatch preserves Hyper's existing wire form. The pool inserts `Host` when configured and absent,
using the non-default port when one exists. `CONNECT` uses authority form, a request sent to a cleartext HTTP
proxy uses absolute form, and a direct or tunneled request uses origin form. H2 receives the form Hyper expects
for its codec. The retained absolute URI, not the temporary wire form, is the authority for retry, diagnostics,
and error return.

Before readiness or call, the request's `CaptureSmithyConnection` backchannel is bound to metadata for the
selected physical connection: proxy state, local and remote addresses, and a poison callback naming that exact
connection generation. A stale-reuse retry replaces the binding with the replacement connection. The
callback is idempotent, becomes a no-op after its generation is gone, and does not keep a retired connection
or the pool alive. Connection metadata attached by the connector is also copied to the response before the
response head is exposed.

#### Readiness and Hyper acceptance

The selected H1 guard owns its exclusive sender; a selected H2 request lease owns a sender for one prospective
stream. The same mutable sender instance is polled ready and then called with `try_send_request`. There is no
published `Ready` state and no handoff between those operations: the poll that observes readiness invokes
`try_send_request` before returning. Hyper polling, request-body polling, callbacks, wakes, and destructive
drops all occur outside pool locks.

Readiness alone does not authorize dispatch. After readiness succeeds, a per-record H1 gate or
per-generation H2 gate commits the dispatch against logical close. Dispatch commit and logical close are
mutually exclusive linearization points implemented without holding a pool lock through Hyper. If close wins,
the request remains locally owned and the selected H1 guard or prospective H2 lease follows its pre-call
cleanup. If dispatch commit wins, close accounts for the request as an in-flight dispatch while
`try_send_request` follows immediately in the same poll. Hyper may still return the original request unsent;
that returned request remains the only retry authority, while the closing connection cannot accept new work.

```text
Acquired
  -> Prepared
  -> PollingReady
       +-- cancelled or closed before call -> release selected guard; request remains unsent
       `-- ready
            -> DispatchCommit races logical close
                 +-- close wins -> release selected guard; request remains unsent
                 `-- commit wins
                       -> Calling try_send_request on the same sender
                            +-- Hyper returns original request -> UnsentReturned
                            +-- Hyper accepts request ---------> WaitingForHeaders
                                                                   +-- error -> TerminalError
                                                                   `-- head  -> BodyGuardTransferred
```

Selection is provisional until readiness succeeds. If readiness reports a closed connection before call, the
pool retires that stale selection and continues acquisition with the still-owned request. Once
`try_send_request` accepts the request envelope, Hyper owns the request and its body. That point discharges any
H2 generation-gate opportunity; selecting or cloning a sender is not enough. The request future continues to
own the Hyper response future, the H1 checked-out guard or H2 receive endpoint, and a strong pool reference
while it waits for response headers; an accepted H2 request-body adapter owns the matching send endpoint.

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

| Stage              | Request                   | Dispatch handle                               | Connection / stream guard                                              | Response body            | Retry authority        |
| ------------------ | ------------------------- | --------------------------------------------- | ---------------------------------------------------------------------- | ------------------------ | ---------------------- |
| Acquiring          | Request future            | None                                          | Waiter or delivery fallback                                            | None                     | None                   |
| Prepared / ready   | Request future            | Selected sender                               | H1 checked-out guard or prospective H2 lease                           | None                     | None                   |
| Unsent returned    | Request future regains it | Stale sender retires                          | Guard resolves; H2 endpoint is inert                                   | None                     | Reused connection only |
| Accepted / headers | Hyper                     | H1 request future; H2 local handle releasable | H1 request future; H2 send and receive endpoints                       | None                     | None                   |
| Headers delivered  | Consumed                  | H1 body guard; no H2 local handle             | Body or upgrade owns H1 or H2 receive; request adapter may own H2 send | Caller owns guarded body | None                   |
| Terminal           | None                      | H1 source or retired; H2 generation           | H1 returned or closing; H2 lease released                              | Completed or dropped     | None                   |

The open connection record owns its capacity lease throughout this table. Dispatch never moves the permit into
the request, sender, body, or request lease; only logical close returns it to admission.

Dropping during acquisition uses the waiter, delivery, and refunnelling rules already defined. Dropping after
selection but before call returns a still-usable H1 through its source's ordinary return path or releases the
H2 request lease. Dropping after Hyper accepts but before headers closes H1 through Hyper's supported
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

An upgrade changes which object owns protocol completion. H1 drivers run with upgrade support. Before the
request future exposes a response carrying Hyper's `OnUpgrade`, it marks the checked-out guard upgrade-pending,
so an empty response body cannot take the ordinary H1 return path. The response and driver then share one
terminal transition: response cancellation or driver failure logically closes the connection; driver
commitment logically closes it, removes its sender from the pool, and releases its capacity lease. Hyper
transfers the wrapped transport and any bytes read past the HTTP message into the
`Upgraded` object. The caller then owns the upgraded I/O; dropping it signals physical completion through the
transport wrapper. A response or `OnUpgrade` dropped before transfer instead lets Hyper close that transport.
An upgraded H1 is never returned as an HTTP connection.

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

* **Same-instance dispatch** [safety] — one selected sender is polled ready and called without publishing or
  transferring an intermediate ready state, in the poll that observes readiness.
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

For H1, end-of-stream begins return processing; the checked-out sender returns to its source only after Hyper
also reports it ready for another request. Dropping an incomplete body is not itself evidence of reusability.
Hyper may synchronously consume an already-buffered remainder and prove the message boundary; if it does, the
same ready check may return the connection. If the remainder is unavailable, cancellation, body error, or
protocol state cannot prove the boundary, the connection logically closes. The pool does not parse or drain
HTTP independently of Hyper.

The response path polls H1 readiness once. If readiness is pending after the response reaches a reusable
protocol boundary, it transfers the exclusive sender and return cleanup to an `H1ReturnTask` spawned through
the connection's owner-partition `DriverSpawner`. The response body does not retain responsibility for polling
that sender, and `Drop` never waits. The task enters source return only after Hyper proves both the message
boundary and readiness for another request. Closed, poisoned, upgraded, or owner-runtime-shutdown outcomes
logically close the record; dropping the task owns the same source-close fallback.

For H2, body end-of-stream or a stream-local error terminates the receive endpoint. Dropping an incomplete
body does the same and asks Hyper to send `RST_STREAM(CANCEL)`; the lease releases after the
request-body send endpoint also terminates. Neither outcome retires an otherwise healthy generation. GOAWAY,
connection failure, or explicit poisoning may independently have moved the generation to draining, in which
case the last lease completes drain instead of returning it to accepting
state. An H2 extended `CONNECT` follows its upgrade lifecycle bridge rather than the ordinary body terminal.

Every H1 return revalidates the record's generation, poison state, and idle policy under the source cell
lock. An unbounded origin serves compatible local demand or installs the connection as idle directly because it
has no admission state or cross-cell claims. For a bounded origin, the same transition also checks its installed
return claim. An installed claim extracts the sender into a source-owned provisional candidate. Otherwise the
record enters a source-owned `Returning` residency while a lightweight return offer consults admission. The
sender remains named by its H1 record but is not dispatch-eligible while admission compares the source's
compatible-group and origin heads. Admission returns one typed decision to the source: serve compatible local
demand, reclaim locally, install a cross-cell claim, or install the connection as idle when no demand can use
it. Dropping an uncommitted offer restores ordinary source return handling.

`Returning` is counted as active rather than idle because no request may select it. The final source-cell
transition revalidates retirement state before applying the decision, so a body that finishes concurrently
with poison, reclaim, driver failure, or pool shutdown cannot republish a connection after retirement.

#### Two-phase close

A connection that leaves the pool does so in two steps, and the gap between them is deliberate. At **logical
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
  is kept. Closing an otherwise-quiescent connection cannot wait for the next request, so each partition runs
  a maintenance task on its own runtime that wakes on the nearest idle deadline and closes what has expired.
  Every task uses the pool's builder-injected `TimeSource` and `AsyncSleep`, so tests drive idle age with a fake
  clock. Because that time source is `SystemTime`, which can step backward or forward, idle age is measured
  against the scheduled sleep deadline the task already holds, not by subtracting two `SystemTime` readings —
  the deadline gives a monotonic floor, and a clock that jumps changes when the task wakes but not whether a
  connection idle since a fixed deadline has expired. The task shuts down with its partition, and maintenance
  stays off the request path — a checkout never scans for expired connections.
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
  request: it closes a connection that is idle now, or claims one as it returns from its current request and
  closes it before it serves another. It does not abort active work to free a permit.
* **Pool or owner-runtime shutdown** — pool drop logically closes every remaining record. If an owning runtime
  drops a connection's guarded driver task, the driver lifecycle guard requests logical close with
  `OwnerRuntimeShutdown`; dropping the driver also closes its root transport unless an H1 upgrade already
  transferred that transport. Per-stream and upgrade tasks use their request-lifecycle cleanup and do not by
  themselves close a healthy H2 connection. Outstanding request and body guards run their normal terminal
  cleanup, and every close request races through the same exactly-once transition.

The first trigger to begin logical close removes reuse eligibility, releases capacity, and records the close
reason. Later triggers observe that terminal transition and cannot release capacity or report close again.
`Poisoned` is reserved for an explicit poison signal; `ProtocolClosed` is reserved for independently observed
connection-level termination. The close event carries the source error when one exists. Concurrent signals
still race through first-trigger-wins, but one initiating signal does not match both categories. Every reason
ends at the same physical completion, so capacity and lifecycle accounting do not depend on what won the race.

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
* **Return revalidation** [safety] — an H1 return checks generation and retirement state under its source cell
  before becoming visible; a sender awaiting admission remains source-owned and non-dispatchable in
  `Returning`, so a late completion or decision cannot reverse logical close or bypass return ordering.
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
`ConnectionBorrowedEvent` is the successful H1 return-claim transfer from source to target. Cross-partition
H2 selection is visible as a reuse whose owner and request partitions differ, preserving *borrow* as the H1
mechanism defined above.
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
close, not physical teardown, and retains the first reason recorded by that transition. Establishment failure
is emitted after its capacity and waiter state have been reconciled. All callbacks observe committed pool
state.

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
`h1_idle` and `h1_active` partition logically open H1 records; checked-out and `Returning` H1 records are
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
* **Single close report** [safety] — logical close records one write-once reason and invokes at most one close
  callback for a connection identity.
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

**Source** and **target** — roles in a cross-cell return claim: the source cell holds a reusable HTTP/1
connection, and the target cell wants its handle or permit.

**Permit** — the conserved unit of connection capacity. It has exactly one owner at a time and is moved or
released, never copied. *Capacity* is the aggregate quantity permits account for, used in sums and bounds.

**Demand** — a cell's standing signal that it could use one more connection. One fixed ticket per cell,
not one per request, so demand cannot accumulate.

**Borrow** — moving a dispatch handle to a peer cell, leaving the connection open. Transfers no I/O
authority.

**Reclaim** — closing a connection so its permit can move to another cell. Transfers capacity, not I/O.

**Capacity lease**, **request lease**, and **handle** — a capacity lease is the exclusive hold on one permit
and moves from establishment to the connection record. An H2 request lease owns one prospective or accepted
stream's two-ended lifecycle but no permit. A dispatch handle can address a connection and owns neither kind of
capacity.

**Retry authority** — proof that the same request may be dispatched again. Only Hyper returning the original
request unsent from a reused connection creates this authority; request clonability does not.

**Publish** and **deliver** — publication makes state visible to many unnamed readers; delivery hands one
value to one waiting party. `DeliveryState` tracks a ticket's residence and acknowledgement fence;
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

**Revision** — the identity of one demand episode. A **version** orders complete published snapshots for that
revision; readers retain the newest and discard older snapshots, while work for a retired revision is stale.

**Episode** — a bounded activity admitting at most one terminal outcome. Work naming a superseded episode
is rejected.

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
*Enforced by:* Bounded demand and Snapshot ordering identify the live episode; Single delivery retains its
fence through acknowledgement; Refunnelling and Acknowledged progress give every rejection and drop a terminal
path. An H2 generation is not a one-to-one resource: its source record retains capacity while generation
identity is published to compatible local waiters and announced to eligible peer cells in bounded turns.

**No committed waiter starves.** An eligible committed waiter is served whenever a permit it may use becomes
reachable, and is not passed indefinitely by later arrivals. *Rules out:* unbounded overtaking; a resource
sitting idle while an eligible waiter waits; capacity stranded on a source connection that returns reusable
without ever going observably idle; a newly published H2 generation serving newer local arrivals while older
compatible waiters remain parked. *Enforced by:* Cross-cell order and Bounded overtaking (the oldest eligible
residence comes from a stored head), Return interception and Source fairness turn (a returning connection
reaches an older peer without starving the source), Publication priority (the generation gate serves committed
local waiters before newer arrivals), and Work-conserving service, with Bounded grant work bounding the
coordination cost and Bounded peer discovery preventing source searches from growing with partition count.
This holds only while progress is possible — it is not promised while every permit for the origin is held
indefinitely by active HTTP/2 work that the waiter is not eligible to use, a limit stated under
[Eligible requests make progress](#eligible-requests-make-progress).

**Observation cannot corrupt pool state.** Listener code runs outside pool locks and only after its triggering
transition is complete, so pool invariants do not depend on a listener succeeding. *Rules out:* a listener
observing or holding partially transitioned state; a panicking listener leaving committed state inconsistent;
a listener blocking coordination by retaining a pool lock. *Does not rule out:* a listener delaying or
ending the task that invokes it, or delaying work sequenced after its return. In particular, the creation
callback is intentionally a barrier before request visibility. *Enforced by:* Report locality, Panic
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

### Transfer-based explicit-partition establishment

An explicit partition's client is initially required to be driven from the runtime named by its
`DriverSpawner`. Driving it from another independent runtime is unsupported; the consequence is a caller
contract rather than an extra spawn and wake on every establishment. The anonymous default does not have this
explicit-client affinity precondition: it binds one runtime on first establishment and already moves freely
among that runtime's worker threads.

A future implementation can transfer an unpolled connector, TLS, and Hyper-handshake task to the explicit
partition's spawner and return the result through a one-shot delivery. That would remove the explicit-client
affinity precondition, but it also adds cancellation and result-refunnelling ownership plus a cross-runtime
wake to the create path. Add it when a concrete caller must move one explicit client among independent Tokio
runtimes, not merely among worker threads of one runtime.

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
    pub fn new(pool: &ConnectionPool) -> Result<Self, InvalidPartition>;
    pub fn from_partition(
        pool: &ConnectionPool,
        id: PartitionId,
    ) -> Result<Self, InvalidPartition>;
}

#[derive(Debug)]
pub struct InvalidPartition { /* private */ }

impl InvalidPartition {
    pub fn partition(&self) -> PartitionId;
}
```

`Client` implements the smithy runtime's `HttpClient` through the
[Smithy client boundary](#smithy-client-boundary): each returned HTTP connector carries operation policy while
sharing this client's pool and resolved partition. `Client::new` resolves `PartitionId::ANONYMOUS`; it succeeds
only for a pool built without explicit partitions. `Client::from_partition` resolves the supplied identity,
including the anonymous identity when it exists. Either returns `InvalidPartition` rather than panicking when
the pool has no such partition. Resolution happens once at client construction, so a request performs no
partition lookup. `InvalidPartition` implements `Error` and reports the unresolved identity.

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
`max_connections_per_host` is unset by default, and an unset bound constructs no admission machinery; when
set, it bounds one origin across all partitions and interface groups, not per partition.

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
    client.rs          — Client, PoolConnector, and InvalidPartition
    partition.rs       — partition declarations and runtime/interface placement
    origin.rs          — owned OriginKey, borrowed lookup, and canonicalization
    registry.rs        — PartitionRegistry, PartitionState, and stable cell publication
    cell.rs            — OriginCell, local selection, waiters, H1/H2 residency
    admission.rs       — permits, demand orders, return claims, delivery
    handshake.rs       — HTTP/1 attempts, HTTP/2 flights, ALPN convergence
    dispatch.rs        — request preparation, Hyper dispatch, response guards
    connection.rs      — records, leases, logical close, physical completion
    events.rs          — listener and lifecycle event types
    stats.rs           — origin/partition snapshots and lifecycle gauges
aws-smithy-http-client/src/sync/
                       — standard-library and Loom synchronization facade
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
implementation-neutral connection harness and existing-client behavior suites landed in smithy-rs
[PR #4767](https://github.com/smithy-lang/smithy-rs/pull/4767) and form the compatibility baseline. Pool-specific
tests preserved on `archive/conn-pool-4708` are an additional inventory, not an acceptance target: applicable
contracts may be retained or rewritten, implementation-specific assumptions may be obsolete, and the owned
state machine requires coverage that prototype tests did not contain.

The evidence levels have distinct jobs:

* **Unit and property tests** cover pure construction, canonicalization, indexing, accounting, and state
  transition functions over broad generated inputs.
* A **deterministic state model** explores bounded interleavings of admission, demand, delivery, claims,
  publication, cancellation, and shutdown. It is the primary evidence for complete transition coverage and
  progress when a usable resource exists.
* Focused **Loom kernels** exercise synchronization boundaries where task interleavings or memory ordering can
  violate ownership: first cell creation, permit delivery and refunnelling, return-claim endpoints,
  generation publication, logical close, and response-guard transfer. Loom models these kernels rather than
  the complete network client.
* **Controlled-runtime tests** use injected time, sleep, connectors, and executors to force cancellation at
  each await boundary, guarded-driver drop, runtime shutdown, idle deadlines, same-runtime anonymous movement,
  independent-runtime misuse, explicit placement checks, and connector or handshake failure.
* The **wire harness** verifies HTTP/1.1 and HTTP/2 behavior against scripted peers, including reuse,
  multiplexing, ALPN, GOAWAY, stream reset, incomplete bodies, upgrades, poisoning, and transport close.
* **Differential tests** run the same implementation-neutral behavior contracts against the current
  hyper-util-backed client and this pool. Any difference in request behavior, metadata, timeout scope, or error
  classification requires an explicit design decision rather than a rewritten oracle.
* **Benchmarks and stress tests** establish that the optimization and liveness contracts remain true at
  production concurrency and topology.

The required evidence maps to the architecture as follows:

| Mechanism                                                             | Primary evidence                                                                         | What it must establish                                                                                                                                                                                                                                         |
| --------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Construction, topology, origin identity, and stable cells             | unit/property; allocation instrumentation; controlled runtime; Loom cell-creation kernel | invalid configurations fail; equivalent URIs share one origin; canonical request hits allocate no host storage; each pair has one stable cell; the anonymous partition binds one runtime but moves across its workers; explicit placement follows its contract |
| Smithy `HttpClient` boundary and operation policy                     | unit; controlled runtime; differential                                                   | settings-specific facades share one pool and admission authority; request version does not split pool or admission identity; timeout scope, maintenance ownership, validation timing, and `hyper/1.x` metadata are preserved                                   |
| Local reuse, establishment, ALPN convergence, and generation identity | unit/property; deterministic model; controlled runtime; wire; differential               | local hits avoid origin-wide coordination; connector readiness and placement hold; one H2 flight/generation wins; losing transports, leases, and waiters terminate exactly once                                                                                |
| Admission, demand revisions, and origin/group ordering                | property; deterministic model; Loom scheduling kernels; stress                           | the bound is never exceeded; stale snapshots cannot resurrect demand; each resource uses the correct scheduling scope; eligible committed demand has bounded overtaking                                                                                        |
| Capacity delivery, H1 return claims, and source turns                 | deterministic model; Loom delivery/claim kernels; controlled cancellation                | every permit and provisional H1 has one owner; candidate transfer revalidates source state; acknowledgement fences close; cancellation and task drop refunnel once; return interception cannot starve source-local demand                                      |
| H2 publication and request leases                                     | deterministic model; Loom publication kernel; wire                                       | publication moves no capacity; generation gates prioritize committed waiters; stale generations cannot dispatch; send and receive endpoints both terminate before lease release                                                                                |
| Dispatch, retry, bodies, upgrades, and metadata                       | controlled runtime; wire; differential                                                   | readiness and call use one sender; only Hyper-certified unsent reuse retries; cancellation has a stage-local owner; H1 framing and H2 stream isolation hold; existing metadata and error behavior are preserved                                                |
| Logical and physical close, maintenance, events, and statistics       | unit/property; Loom close/guard kernels; controlled time/runtime; wire                   | driver completion and cancellation request logical close; permit release occurs once; root-I/O drop ends physical accounting; idle deadlines and shutdown clean up; callbacks see committed state and gauges converge to lifecycle state                       |
| Locality, liveness, tails, topology scaling, and retained memory      | deterministic model; repeated stress; benchmarks                                         | grant work is independent of partition count; no reachable resource remains idle behind demand; local reuse does not regress; topology scales without moving I/O; physical-socket and route-memory costs are measured                                          |

Correctness acceptance requires every applicable unit, model, Loom, controlled-runtime, wire, and
differential suite to pass. The deterministic model must explore cancellation and terminal outcomes from
every protocol state and report neither an invalid state nor a reachable nonterminal state with usable
capacity and no enabled progress action. Concurrency-sensitive suites run repeatedly in CI; a flaky failure is
a correctness failure, not benchmark noise.

Performance acceptance uses comparison gates, with environment-specific numeric thresholds stored beside the
benchmark configuration rather than in this design. Local H1 return and H2 activation must not regress
against the current hyper-util-backed client. The cap profiles that previously produced approximately 45.1-second
and 23.2-second P999 tails are reproduced to test bounded overtaking under pressure. The established 300 Gb/s
single-NIC and 600 Gb/s dual-NIC profiles are repeated to verify balanced interface use and owner-partition
I/O placement. H2 runs record the one-generation throughput cliff before stream-credit pooling is considered.
Lifecycle and memory runs record peak physical-socket excess during slow teardown and retained route metadata
across realistic partition-by-origin cardinalities.

---

## Appendix C: FAQ

### Why not build the pool from composable connector layers?

Hyper's ecosystem offers pooling as connector middleware — a cache layer, a connection-limit layer, a
negotiate layer, each a `Service` wrapping the one below. Assembling those rather than owning the
coordination layer is the obvious alternative. The pool owns it instead because the coordination the pool
needs is not local to any one layer, and stacked layers give no layer the whole picture.

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

This is not a hypothetical objection. The
[earlier composable-pool prototype](https://github.com/smithy-lang/smithy-rs/pull/4708) had to vendor the cache
layer and carry SDK-specific modifications, so the layering was being fought rather than used. Owning the
cache, limit, and negotiate layers as one unit is the decision to design them together against the lifecycle
they share instead of reconciling three independent views of it after the fact. The cost is taken deliberately:
the pool forgoes future upstream improvements to those layers, so its equivalents must be as strong or
stronger. What it does not touch is the connector contract below it or Hyper's protocol implementation above
it — both are kept unchanged, because that is where the ecosystem integrates.
