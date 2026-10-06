"""Seed requests for the A/B fuzz campaign: valid and near-valid requests for every operation.

The test models carry no `@httpRequestTests`, so fuzzgen's lexicon has an empty corpus. Without seeds
AFL would have to discover the request encoding, the routes and the wire format on its own.

`seeds(suite, protocol)` returns [(label, uri, method, {header: [values]}, body_bytes)].
"""
import json
import struct
import zlib

# operation -> (method, uri, {header: value}, body members, well-formed variations of the members)
# For the REST protocols the members named in uri, headers and query are bound there; RPC protocols
# carry every member in the body.
SUITES = {
    "multiprotocol": {
        "service": "MultiProtocolService",
        "operations": {
            "Greet": {
                "method": "POST",
                "uri": "/greet/{name}",
                "query": {"times": "times"},
                "headers": {"greeting": "x-greeting"},
                "inputs": [
                    {"name": "alice", "greeting": "hello", "times": 2, "tags": ["a", "b"]},
                    {"name": "bob"},
                    {"name": "intruder", "tags": []},
                    # Violates @length(min: 1, max: 32).
                    {"name": "x" * 33, "times": -1},
                    {"name": "carol", "times": 2147483648, "tags": ["", "é"]},
                ],
            },
            "Ping": {"method": "GET", "uri": "/ping", "inputs": [{}]},
            "Subscribe": {
                "method": "POST", "uri": "/subscribe",
                "headers": {"topic": "x-topic", "count": "x-count"},
                "inputs": [{"topic": "fuzz", "count": 2}],
            },
        },
    },
    "pokemon": {
        "service": "PokemonService",
        "operations": {
            "GetPokemonSpecies": {
                "method": "GET",
                "uri": "/pokemon-species/{name}",
                "inputs": [{"name": "pikachu"}, {"name": "a b%2Fc"}],
            },
            "GetStorage": {
                "method": "GET",
                "uri": "/pokedex/{user}",
                "headers": {"passcode": "passcode"},
                "inputs": [{"user": "ash", "passcode": "pikachu123"}, {"user": "ash"}],
            },
            "GetServerStatistics": {"method": "GET", "uri": "/stats", "inputs": [{}]},
            "DoNothing": {"method": "GET", "uri": "/do-nothing", "inputs": [{}]},
            "CheckHealth": {"method": "GET", "uri": "/ping", "inputs": [{}]},
        },
    },
}


def cbor(value):
    def head(major, n):
        if n < 24:
            return bytes([major << 5 | n])
        for info, fmt in ((24, ">B"), (25, ">H"), (26, ">I"), (27, ">Q")):
            if n < 1 << (8 << (info - 24)):
                return bytes([major << 5 | info]) + struct.pack(fmt, n)
        raise ValueError(n)

    if isinstance(value, bool):
        return bytes([0xF5 if value else 0xF4])
    if isinstance(value, int):
        return head(0, value) if value >= 0 else head(1, -1 - value)
    if isinstance(value, str):
        raw = value.encode()
        return head(3, len(raw)) + raw
    if isinstance(value, list):
        return head(4, len(value)) + b"".join(cbor(v) for v in value)
    if isinstance(value, dict):
        return head(5, len(value)) + b"".join(cbor(k) + cbor(v) for k, v in value.items())
    raise TypeError(value)


def xml(root, members):
    def element(name, value):
        if isinstance(value, list):
            return f"<{name}>" + "".join(element("member", v) for v in value) + f"</{name}>"
        return f"<{name}>{value}</{name}>"

    return (f"<{root}>" + "".join(element(k, v) for k, v in members.items()) + f"</{root}>").encode()


def _rest(op, spec, members, content_type, encode):
    members = dict(members)
    uri = spec["uri"]
    for label in [part[1:-1] for part in uri.split("/") if part.startswith("{")]:
        uri = uri.replace("{" + label + "}", str(members.pop(label, "")).replace(" ", "%20"))
    headers = {}
    for member, header in spec.get("headers", {}).items():
        if member in members:
            headers[header] = [str(members.pop(member))]
    query = [f"{name}={members.pop(member)}" for member, name in spec.get("query", {}).items() if member in members]
    if query:
        uri += "?" + "&".join(query)
    out = []
    if members or spec["method"] != "GET":
        out.append((uri, spec["method"], {**headers, "content-type": [content_type]}, encode(op, members)))
        if not members:
            out.append((uri, spec["method"], headers, b""))
    else:
        out.append((uri, spec["method"], headers, b""))
        # A body-less request that still names its protocol.
        out.append((uri, spec["method"], {**headers, "content-type": [content_type], "accept": [content_type]}, b""))
    return out


def _requests(service, protocol, op, spec, members):
    as_json = json.dumps(members).encode()
    if protocol == "rest-json1":
        return _rest(op, spec, members, "application/json", lambda _, m: json.dumps(m).encode())
    if protocol == "rest-xml":
        return _rest(op, spec, members, "application/xml", lambda o, m: xml(o + "Input", m))
    if protocol in ("aws-json-10", "aws-json-11"):
        content_type = "application/x-amz-json-1." + protocol[-1]
        headers = {"content-type": [content_type], "x-amz-target": [f"{service}.{op}"]}
        out = [("/", "POST", headers, as_json)]
        if not members:
            out.append(("/", "POST", headers, b""))
        return out
    if protocol == "rpcv2-cbor":
        uri = f"/service/{service}/operation/{op}"
        headers = {"smithy-protocol": ["rpc-v2-cbor"], "accept": ["application/cbor"]}
        with_body = {**headers, "content-type": ["application/cbor"]}
        out = [(uri, "POST", with_body, cbor(members))]
        if not members:
            out.append((uri, "POST", headers, b""))
        return out
    raise ValueError(protocol)


def seeds(suite, protocol):
    service = SUITES[suite]["service"]
    out = []
    for op, spec in SUITES[suite]["operations"].items():
        for n, members in enumerate(spec["inputs"]):
            for k, request in enumerate(_requests(service, protocol, op, spec, members)):
                out.append((f"{op}#{n}.{k}", *request))
    out.extend(event_stream_seeds(suite, protocol))
    if suite == "multiprotocol" and protocol in ("aws-json-10", "aws-json-11", "rest-json1"):
        for label, number in [("trailing-decimal", "214748364."),
                              ("leading-zero", "0147483648"),
                              ("leading-zero-cr", "0214748364\r")]:
            body = ('{"name":"carol","times":' + number + '}').encode()
            if protocol == "rest-json1":
                # times is query-bound here: preserve this as a skipped-value
                # grammar regression seed, not a modeled body-integer test.
                out.append((f"F2/skipped-{label}", "/greet/carol", "POST",
                            {"content-type": ["application/json"]}, body))
            else:
                out.append((f"F2/{label}", "/", "POST",
                            {"content-type": ["application/x-amz-json-1." + protocol[-1]],
                             "x-amz-target": [f"{service}.Greet"]}, body))
    # Requests no protocol should route.
    out.append(("unknown route", "/nope", "GET", {}, b""))
    out.append(("unknown route with body", "/nope", "POST", {"content-type": ["application/json"]}, b"{}"))
    return out


def event_frame(headers, payload):
    """Encode string headers and both CRCs of an Amazon event-stream message."""
    encoded = b""
    for name, value in headers.items():
        name, value = name.encode(), value.encode()
        encoded += bytes([len(name)]) + name + b"\x07" + struct.pack(">H", len(value)) + value
    prelude = struct.pack(">II", 16 + len(encoded) + len(payload), len(encoded))
    message = prelude + struct.pack(">I", zlib.crc32(prelude)) + encoded + payload
    return message + struct.pack(">I", zlib.crc32(message))


def event_stream_seeds(suite, protocol):
    rpc = protocol in ("aws-json-10", "aws-json-11", "rpcv2-cbor")
    content_type = "application/cbor" if protocol == "rpcv2-cbor" else (
        "application/xml" if protocol == "rest-xml" else "application/json"
    )

    def payload(root, members):
        if protocol == "rpcv2-cbor":
            return cbor(members)
        if protocol == "rest-xml":
            return xml(root, members)
        return json.dumps(members).encode()

    def frame(kind, name, data):
        if kind == "exception" and protocol == "rest-xml":
            data = b"<ErrorResponse>" + data + b"</ErrorResponse>"
        return event_frame({
            ":message-type": kind,
            ":exception-type" if kind == "exception" else ":event-type": name,
            ":content-type": content_type,
        }, data)

    if suite == "multiprotocol":
        operation, uri = "Publish", "/publish"
        initial = {}
        headers = {}
        event = frame("event", "note", payload("Note", {"text": "hello"}))
        exception = frame("exception", "UnknownError", payload("Error", {"message": "failed"}))
    else:
        operation, uri = "CapturePokemon", "/capture-pokemon-event"
        initial = {}
        headers = {}
        # CapturingEvent's explicit structure payload is encoded directly.
        event = frame("event", "event", payload("CapturingPayload", {"name": "pikachu", "pokeball": "master"}))
        exception = frame("exception", "masterball_unsuccessful", payload("Error", {"message": "failed"}))
    service = SUITES[suite]["service"]
    prefix = frame("event", "initial-request", payload(operation + "Input", initial)) if rpc and initial else b""
    if protocol.startswith("aws-json"):
        uri = "/"
        headers = {"x-amz-target": [f"{service}.{operation}"]}
    elif protocol == "rpcv2-cbor":
        uri = f"/service/{service}/operation/{operation}"
        headers = {"smithy-protocol": ["rpc-v2-cbor"]}
    headers["content-type"] = ["application/vnd.amazon.eventstream"]
    bad_crc = event[:-1] + bytes([event[-1] ^ 1])
    variants = [
        ("empty", b""), ("one", event), ("multiple", event + event),
        ("exception", event + exception), ("bad-crc", event + bad_crc),
        ("truncated", event + event[:-3]),
        ("unknown-event", frame("event", "unknown", payload("Unknown", {}))),
        ("bad-payload", frame("event", "note" if suite == "multiprotocol" else "event", b"\xff")),
    ]
    out = [(f"{operation}/{label}", uri, "POST", headers, prefix + body) for label, body in variants]
    if rpc:
        unexpected_initial = frame("event", "initial-request", payload(operation + "Input", {}))
        out.append((f"{operation}/unexpected-initial", uri, "POST", headers, unexpected_initial + event))
    return out
