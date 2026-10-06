#!/usr/bin/env python3
"""Seeding and triage for the single-protocol versus multi-protocol A/B fuzz campaign.

A "case" is one comparison: <suite>/<case>, for example multiprotocol/rest-json1. Its work directory is
$FUZZ_ROOT/work-isolated-v1/<suite>/<case> (FUZZ_ROOT defaults to ./artifacts next to this file).

  seed <suite> <case>             Write the seed corpus (seeds.py) into the case's afl-input/corpus.
  corpus-diff <suite> <case>      Send every seed to both targets and report the ones that differ.
  replay <suite> <case>           Send every saved crash and hang to both targets, write replay.jsonl,
                                  and group the divergences. Hangs are usually divergences too: on a
                                  mismatch the driver re-runs both targets 10 times, which under load
                                  exceeds AFL's timeout.
  cluster <suite> <case> [-v N]   Group the results in replay.jsonl again; -v prints N samples per group.
  probe <suite> <case> <cases.py> Send hand-written requests. cases.py defines
                                  CASES = [(label, uri, method, {header: [values]}, body_bytes)].
  dump <file>                     Print the exact request stored in an AFL input file.

Every command compares the two targets the way the driver does. With SMITHY_FUZZ_SEMANTIC_COMPARE set
(the default in env.sh) that is: the deserialized input, the status, the headers except
`content-length`, and the body as a JSON or CBOR value or as XML with differently-named siblings in any
order. Otherwise: the response, byte for byte. With SMITHY_FUZZ_IGNORE_UNROUTED explicitly set (disabled by default),
the targets also agree when the single-protocol target did not route the request to an operation (404
or 405, no handler invoked), whatever the multi-protocol target answers, and when the single-protocol
target rejected the request with a 4xx before invoking a handler and the multi-protocol target did
not route it.
"""
import collections
import glob
import json
import os
import struct
import subprocess
import sys
import tempfile
import zlib
import xml.etree.ElementTree as ET

HERE = os.path.dirname(os.path.abspath(__file__))
FUZZ_ROOT = os.environ.get("FUZZ_ROOT", os.path.join(HERE, "artifacts"))
SEMANTIC = bool(os.environ.get("SMITHY_FUZZ_SEMANTIC_COMPARE", "1"))
IGNORE_UNROUTED = bool(os.environ.get("SMITHY_FUZZ_IGNORE_UNROUTED", ""))
SINGLE, MULTI = "single", "multi"

# ---------------------------------------------------------------------------------------------
# Wire formats


def _lp(b):
    return struct.pack("<Q", len(b)) + b


def encode_request(uri, method, headers, body):
    """bincode 1.x encoding of aws_smithy_fuzz::HttpRequest {uri, method, headers, trailers, body}."""
    out = _lp(uri.encode()) + _lp(method.encode()) + struct.pack("<Q", len(headers))
    for k, vs in headers.items():
        out += _lp(k.encode()) + struct.pack("<Q", len(vs))
        out += b"".join(_lp(v.encode() if isinstance(v, str) else v) for v in vs)
    return out + struct.pack("<Q", 0) + _lp(body)


def decode_request(data):
    i = 0

    def u64():
        nonlocal i
        v = struct.unpack_from("<Q", data, i)[0]
        i += 8
        return v

    def raw():
        nonlocal i
        n = u64()
        v = data[i:i + n]
        i += n
        return v

    uri = raw().decode("utf-8", "replace")
    method = raw().decode("utf-8", "replace")
    headers = {}
    for _ in range(u64()):
        k = raw().decode("utf-8", "replace")
        headers[k] = [raw() for _ in range(u64())]
    for _ in range(u64()):
        raw()
        [raw() for _ in range(u64())]
    return uri, method, headers, raw()


def cbor_canonical(data):
    """A rendering of one CBOR item that is the same for every encoding of the same value."""
    i = 0

    def head():
        nonlocal i
        major, info = data[i] >> 5, data[i] & 31
        i += 1
        if info < 24:
            return major, info, info
        if info in (24, 25, 26, 27):
            n = 1 << (info - 24)
            v = int.from_bytes(data[i:i + n], "big")
            if len(data) < i + n:
                raise ValueError("truncated")
            i += n
            return major, info, v
        if info == 31:
            return major, info, None
        raise ValueError("reserved")

    def take(n):
        nonlocal i
        if len(data) < i + n:
            raise ValueError("truncated")
        v = data[i:i + n]
        i += n
        return v

    def at_break():
        nonlocal i
        if data[i] == 0xFF:
            i += 1
            return True
        return False

    def item():
        major, info, v = head()
        if major == 0:
            return str(v)
        if major == 1:
            return str(-1 - v)
        if major in (2, 3):
            if v is None:
                chunks = b""
                while not at_break():
                    m, _, n = head()
                    if m != major or n is None:
                        raise ValueError("bad chunk")
                    chunks += take(n)
                raw = chunks
            else:
                raw = take(v)
            return ("h" if major == 2 else "t") + raw.hex()
        if major == 4:
            items = []
            if v is None:
                while not at_break():
                    items.append(item())
            else:
                items = [item() for _ in range(v)]
            return "[" + ",".join(items) + "]"
        if major == 5:
            entries = []
            if v is None:
                while not at_break():
                    entries.append(item() + ":" + item())
            else:
                entries = [item() + ":" + item() for _ in range(v)]
            return "{" + ",".join(sorted(entries)) + "}"
        if major == 6:
            return f"{v}({item()})"
        if info == 25:
            return repr(struct.unpack(">e", v.to_bytes(2, "big"))[0])
        if info == 26:
            return repr(struct.unpack(">f", v.to_bytes(4, "big"))[0])
        if info == 27:
            return repr(struct.unpack(">d", v.to_bytes(8, "big"))[0])
        return f"simple({v})"

    out = item()
    if i != len(data):
        raise ValueError("trailing bytes")
    return out


def xml_canonical(data):
    """The document with every element's children stably sorted by name: structure members in any
    order compare equal, list members and map entries keep theirs."""
    def sort(element):
        children = sorted(element, key=lambda child: child.tag)
        for child in children:
            sort(child)
        element[:] = children
        return element

    return ET.tostring(sort(ET.fromstring(data.decode())))


# ---------------------------------------------------------------------------------------------
# Running the targets


class Case:
    def __init__(self, suite, name):
        self.suite, self.name = suite, name
        self.dir = os.path.join(FUZZ_ROOT, "work-isolated-v1", suite, name)
        config = os.path.join(self.dir, "smithy-fuzz-config.json")
        if not os.path.exists(config):
            sys.exit(f"{suite}/{name}: {self.dir} is not initialized (run init.sh)")
        self.libraries = {}
        with open(config) as source:
            self.config = json.load(source)
        for target in self.config["targets"]:
            self.libraries[os.path.basename(target["source"].rstrip("/"))] = target["shared_library"]

    def invoke_file(self, path):
        """{side: result}, where result is None for a panic or hang, else a dict with the exact bytes."""
        if self.config.get("protocol") != self.name:
            raise ValueError("campaign protocol is missing or mismatched; rerun init.sh")
        out = {}
        for side, library in self.libraries.items():
            try:
                res = subprocess.run(
                    ["aws-smithy-fuzz", "invoke-test-case", "--shared-library-path", library, "--test-case", path],
                    cwd=self.dir, capture_output=True, timeout=30,
                    env={**os.environ, "SMITHY_FUZZ_PROTOCOL": self.name},
                )
                r = json.loads(res.stdout)
                out[side] = {
                    "status": r["response"]["status"],
                    "headers": r["response"]["headers"],
                    "body": bytes(r["response"]["body"]),
                    "input": r["input"],
                }
            except subprocess.TimeoutExpired:
                out[side] = {"failure": "hang (>30s)"}
            except Exception:
                stderr = res.stderr.decode("utf-8", "replace")
                panic = [l for l in stderr.splitlines() if "panicked" in l]
                out[side] = {"failure": "panic: " + (panic[0] if panic else stderr[-300:])}
        return out[SINGLE], out[MULTI]

    def invoke(self, uri, method, headers, body):
        with tempfile.NamedTemporaryFile(dir=self.dir, suffix=".probe", delete=False) as f:
            f.write(encode_request(uri, method, headers, body))
        try:
            return self.invoke_file(f.name)
        finally:
            os.unlink(f.name)


def _json_value(data):
    # Python otherwise equates True with 1 and 1 with 1.0, unlike serde_json.
    def typed(value):
        if isinstance(value, dict):
            return {k: typed(v) for k, v in value.items()}
        if isinstance(value, list):
            return [typed(v) for v in value]
        return (type(value).__name__, value)

    def invalid(value):
        raise ValueError(f"invalid JSON constant: {value}")

    return typed(json.loads(data, parse_constant=invalid))


def _valid_content_length(result):
    return all(k.lower() != "content-length" or
               (v.isascii() and v.isdigit() and int(v) == len(result["body"]))
               for k, v in result["headers"].items())


def _headers(r):
    return {k.lower(): v for k, v in r["headers"].items() if not SEMANTIC or k.lower() != "content-length"}


def _bodies_agree(a, b):
    if a == b:
        return True
    if not SEMANTIC:
        return False
    try:
        return _json_value(a) == _json_value(b)
    except Exception:
        pass
    try:
        return cbor_canonical(a) == cbor_canonical(b)
    except Exception:
        pass
    try:
        return xml_canonical(a) == xml_canonical(b)
    except Exception:
        pass
    try:
        left, right = event_frames(a), event_frames(b)
        if len(left) != len(right):
            return False
        for (lh, lp), (rh, rp) in zip(left, right):
            if lh != rh:
                return False
            content_type = next((value for name, kind, value in lh if name == b":content-type" and kind == 7), None)
            try:
                if content_type == b"application/json":
                    equal = _json_value(lp) == _json_value(rp)
                elif content_type == b"application/cbor":
                    equal = cbor_canonical(lp) == cbor_canonical(rp)
                elif content_type in (b"application/xml", b"text/xml"):
                    equal = xml_canonical(lp) == xml_canonical(rp)
                else:
                    equal = lp == rp
            except (ValueError, ET.ParseError):
                equal = lp == rp
            if not equal:
                return False
        return True
    except (ValueError, IndexError, struct.error):
        return False


def event_frames(data):
    """Validate CRCs and return ordered frames with typed headers and raw payloads."""
    frames = []
    while data:
        if len(data) < 16:
            raise ValueError("truncated event-stream frame")
        total, headers_len, prelude_crc = struct.unpack(">III", data[:12])
        if total < 16 or total > len(data) or headers_len > total - 16:
            raise ValueError("invalid event-stream lengths")
        frame, data = data[:total], data[total:]
        if zlib.crc32(frame[:8]) != prelude_crc or zlib.crc32(frame[:-4]) != struct.unpack(">I", frame[-4:])[0]:
            raise ValueError("invalid event-stream CRC")
        raw = frame[12:12 + headers_len]
        headers = []
        while raw:
            size = raw[0]
            name = raw[1:1 + size]
            kind = raw[1 + size]
            raw = raw[2 + size:]
            if kind in (6, 7):
                size = struct.unpack(">H", raw[:2])[0]
                raw = raw[2:]
            else:
                size = {0: 0, 1: 0, 2: 1, 3: 2, 4: 4, 5: 8, 8: 8, 9: 16}.get(kind)
                if size is None:
                    raise ValueError("invalid header type")
            if len(raw) < size:
                raise ValueError("truncated header")
            headers.append((name, kind, raw[:size]))
            raw = raw[size:]
        headers.sort(key=lambda h: h[0])
        frames.append((headers, frame[12 + headers_len:-4]))
    return frames


def pin_protocol(request, protocol):
    """Mirror the native driver's effective protocol identity for replay classification."""
    uri, method, headers, body = request
    cbor_stream = protocol == "rpcv2-cbor" and any(
        k.lower() == "content-type" and any(
            (v.decode("utf-8", "replace") if isinstance(v, bytes) else v).split(";")[0].strip()
            == "application/vnd.amazon.eventstream" for v in vs) for k, vs in headers.items())
    headers = {k: vs for k, vs in headers.items()
               if k.lower() not in ("content-type", "smithy-protocol", "content-encoding")
               and (protocol.startswith("aws-json") or k.lower() != "x-amz-target")}
    media = {"aws-json-10": "application/x-amz-json-1.0", "aws-json-11": "application/x-amz-json-1.1",
             "rest-json1": "application/json", "rest-xml": "application/xml", "rpcv2-cbor": "application/cbor"}
    headers["content-type"] = ["application/vnd.amazon.eventstream" if cbor_stream else media[protocol]]
    if protocol == "rpcv2-cbor":
        headers["smithy-protocol"] = ["rpc-v2-cbor"]
    return uri, method, headers, body


def known_divergence(request, s, m):
    """Recognize only the documented F1, F3 and live-verified X1 signatures."""
    if not SEMANTIC or request is None or "failure" in s or "failure" in m:
        return None
    uri, method, raw_headers, body = request
    path = uri.split("?", 1)[0]
    species = path.removeprefix("/pokemon-species/")
    if (method == "GET" and path.startswith("/pokemon-species/") and species
            and "/" not in species and not body
            and not any(key.lower() == "content-type" for key in raw_headers)
            and isinstance(s.get("input"), str)
            and s["input"].startswith("GetPokemonSpeciesInput {")
            and s["status"] in (200, 404)
            and _valid_content_length(s) and _valid_content_length(m)
            and _headers(s).get("content-type") == "application/xml"
            and m["input"] is None and m["status"] == 404
            and m["body"] == b"<UnknownOperationException/>\n" and not _headers(m)):
        try:
            if xml_canonical(s["body"]) is not None:
                return "X1: restXml Pokemon lookup without Content-Type; strict claiming declines the legacy request"
        except (ValueError, UnicodeError, ET.ParseError):
            pass
    headers = {k.lower(): [v.decode("utf-8", "replace") if isinstance(v, bytes) else v for v in vs]
               for k, vs in raw_headers.items()}
    if (method == "POST" and (path == "/capture-pokemon-event" or
            (path.startswith("/capture-pokemon-event/") and path.removeprefix("/capture-pokemon-event/")
             and "/" not in path.removeprefix("/capture-pokemon-event/")))
            and headers.get("content-type") == ["application/xml"]
            and s["status"] == m["status"] == 200 and _valid_content_length(s)
            and _valid_content_length(m) and _headers(s) == _headers(m)
            and _bodies_agree(s["body"], m["body"])):
        try:
            frames = event_frames(body)
            expected_headers = sorted([(b":message-type", 7, b"exception"),
                (b":exception-type", 7, b"masterball_unsuccessful"),
                (b":content-type", 7, b"application/xml")])
            modeled = '<event-stream-service-error:MasterBallUnsuccessful(MasterBallUnsuccessful { message: Some("failed") })>'
            roots = {
                b"<ErrorResponse><Error><message>failed</message></Error></ErrorResponse>": (modeled, "<event-stream-error>"),
                b"<MasterBallUnsuccessful><message>failed</message></MasterBallUnsuccessful>": ("<event-stream-error>", modeled),
            }
            if len(frames) == 1 and frames[0][0] == expected_headers and frames[0][1] in roots:
                a, b = json.loads(s["input"]), json.loads(m["input"])
                left, right = roots[frames[0][1]]
                if (isinstance(a, list) and isinstance(b, list) and len(a) >= 2
                        and len(a) == len(b) and a[0] == "capture_pokemon"
                        and a[:-1] == b[:-1] and a[-1] == left and b[-1] == right):
                    return "F3: restXml modeled event-error framing; preserve schema shape-root encoding and decoding"
        except (ValueError, TypeError, IndexError, struct.error):
            pass
    if (method != "POST" or not body or s["input"] is not None or m["input"] is not None
            or s["status"] != 400 or m["status"] != 400 or s["body"] != b"response error"
            or not _valid_content_length(s) or not _valid_content_length(m)
            or _headers(s) != _headers(m)):
        return None
    response_headers = _headers(m)
    if set(response_headers) != {"content-type"}:
        return None
    content_type = response_headers["content-type"]
    if content_type in ("application/x-amz-json-1.0", "application/x-amz-json-1.1"):
        if headers.get("content-type") not in ([content_type], ["application/vnd.amazon.eventstream"]):
            return None
        target = headers.get("x-amz-target", [])
        if len(target) != 1 or not target[0].endswith((".Publish", ".CapturePokemon")):
            return None
        expected = b"{}" if content_type.endswith("1.0") else b""
    elif (content_type == "application/cbor"
          and headers.get("smithy-protocol") == ["rpc-v2-cbor"]
          and headers.get("content-type") in (["application/vnd.amazon.eventstream"], ["application/cbor"])
          and uri.endswith(("/operation/Publish", "/operation/CapturePokemon"))):
        expected = b"\xa0"
    else:
        return None
    if m["body"] != expected:
        return None
    try:
        total = struct.unpack(">I", body[:4])[0]
        if total < 16 or total > len(body):
            raise ValueError("invalid first-frame length")
        event_frames(body[:total])
        return None
    except (ValueError, IndexError, struct.error):
        return "F1: corrupt initial event-stream frame; serialization rejection replaces legacy plain-text validation error"


def describe(s, m, request=None):
    """None if the driver would treat the results as agreeing, else what differs."""
    if "failure" in s or "failure" in m:
        return f"single {s.get('failure', 'responded')} / multi {m.get('failure', 'responded')}"
    if known_divergence(request, s, m):
        return None
    if IGNORE_UNROUTED and s["input"] is None:
        unrouted = lambda r: r["input"] is None and r["status"] in (404, 405)
        if unrouted(s) or (unrouted(m) and 400 <= s["status"] < 500):
            return None
    if s["status"] != m["status"]:
        return f"status {s['status']} -> {m['status']}"
    if SEMANTIC and (not _valid_content_length(s) or not _valid_content_length(m)):
        return "invalid content-length"
    if _headers(s) != _headers(m):
        hs, hm = _headers(s), _headers(m)
        names = sorted(k for k in set(hs) | set(hm) if hs.get(k) != hm.get(k))
        return f"status {s['status']}, headers differ: {', '.join(names)}"
    if not _bodies_agree(s["body"], m["body"]):
        return f"status {s['status']}, body differs"
    if SEMANTIC and s["input"] != m["input"]:
        return f"status {s['status']}, deserialized input differs"
    return None


def _short(r):
    if "failure" in r:
        return r["failure"]
    body = r["body"] if len(r["body"]) <= 160 else r["body"][:157] + b"..."
    return f"{r['status']} {r['headers']} {body!r} input={r['input']!r}"


def _print_request(uri, method, headers, body):
    print(f"    {method} {uri!r}\n    headers={headers}\n    body={body[:400]!r}")


# ---------------------------------------------------------------------------------------------
# Subcommands


def protocol_of(case):
    return "rest-xml" if case.startswith("rest-xml") else case


def cmd_seed(suite, case):
    sys.path.insert(0, HERE)
    from seeds import seeds
    campaign = Case(suite, case)
    config_path = os.path.join(campaign.dir, "smithy-fuzz-config.json")
    with open(config_path) as source:
        config = json.load(source)
    config["protocol"] = case
    with open(config_path, "w") as destination:
        json.dump(config, destination, indent=2)
    corpus = os.path.join(campaign.dir, "afl-input", "corpus")
    for old in glob.glob(os.path.join(corpus, "seed-*")):
        os.unlink(old)
    requests = seeds(suite, protocol_of(case))
    for n, (_, uri, method, headers, body) in enumerate(requests):
        with open(os.path.join(corpus, f"seed-{n:03d}"), "wb") as f:
            f.write(encode_request(uri, method, headers, body))
    print(f"{suite}/{case}: wrote {len(requests)} seeds")


def cmd_corpus_diff(suite, case):
    sys.path.insert(0, HERE)
    from seeds import seeds
    c = Case(suite, case)
    requests = seeds(suite, protocol_of(case))
    differ = 0
    statuses = collections.Counter()
    for label, uri, method, headers, body in requests:
        s, m = c.invoke(uri, method, headers, body)
        request = pin_protocol((uri, method, headers, body), c.name)
        known = known_divergence(request, s, m)
        what = describe(s, m, request)
        if known:
            print(f"  KNOWN {label}: {known}")
        statuses[s.get("status", "fail")] += 1
        if what:
            differ += 1
            print(f"  DIFF {label}: {what}")
            _print_request(uri, method, headers, body)
            print(f"    single: {_short(s)}\n    multi:  {_short(m)}")
    print(f"{suite}/{case}: {len(requests)} seeds, {differ} differ; single-target statuses {dict(statuses)}")
    return differ


def _route(uri, headers):
    target = next((v[0].decode("utf-8", "replace") for k, v in headers.items() if k.lower() == "x-amz-target" and v), None)
    path = uri.split("?")[0]
    parts = [p for p in path.split("/") if p]
    if target:
        return target[:60]
    if path.startswith("/service/"):
        return path[:80]
    return "/" + (parts[0][:30] if parts else "")


def cmd_replay(suite, case):
    c = Case(suite, case)
    files = sorted(glob.glob(os.path.join(c.dir, "afl-output", "fuzzer*", "crashes", "id:*"))) + \
        sorted(glob.glob(os.path.join(c.dir, "afl-output", "fuzzer*", "hangs", "id:*")))
    with open(os.path.join(c.dir, "replay.jsonl"), "w") as out:
        for path in files:
            s, m = c.invoke_file(path)
            for r in (s, m):
                if "body" in r:
                    r["body"] = list(r["body"])
            request = pin_protocol(decode_request(open(path, "rb").read()), c.name)
            known = known_divergence(request, {**s, "body": bytes(s.get("body", []))},
                                     {**m, "body": bytes(m.get("body", []))})
            out.write(json.dumps({"path": os.path.relpath(path, c.dir), "single": s, "multi": m,
                                  "known_divergence": known}) + "\n")
    cmd_cluster(suite, case, 0)


def cmd_cluster(suite, case, samples):
    c = Case(suite, case)
    groups = collections.defaultdict(list)
    total = 0
    for line in open(os.path.join(c.dir, "replay.jsonl")):
        o = json.loads(line)
        total += 1
        s, m = o["single"], o["multi"]
        for r in (s, m):
            if "body" in r:
                r["body"] = bytes(r["body"])
        request = decode_request(open(os.path.join(c.dir, o["path"]), "rb").read())
        effective_request = pin_protocol(request, c.name)
        known = known_divergence(effective_request, s, m)
        if known:
            groups[f"KNOWN {known}"].append((o["path"], request, s, m))
            continue
        what = describe(s, m, effective_request)
        if what is None:
            groups["does not diverge on replay"].append((o["path"], None, s, m))
            continue
        groups[f"{request[1][:8]} {_route(request[0], request[2])}: {what}"].append((o["path"], request, s, m))
    print(f"## {suite}/{case}: {total} saved crashes and hangs")
    for name, items in sorted(groups.items(), key=lambda kv: -len(kv[1])):
        print(f"  {len(items):5d}  {name}")
        if not samples or items[0][1] is None:
            continue
        items.sort(key=lambda t: len(t[1][3]) + len(str(t[1][2])) + len(t[1][0]))
        for path, request, s, m in items[:samples]:
            print(f"\n    {path}")
            _print_request(*request)
            print(f"    single: {_short(s)}\n    multi:  {_short(m)}\n")


def cmd_probe(suite, case, cases_file):
    c = Case(suite, case)
    ns = {}
    exec(open(cases_file).read(), ns)
    for name, uri, method, headers, body in ns["CASES"]:
        s, m = c.invoke(uri, method, headers, body)
        request = pin_protocol((uri, method, headers, body), c.name)
        known = known_divergence(request, s, m)
        print(f"{name}\n    single: {_short(s)}\n    multi:  {_short(m)}\n    -> {'KNOWN: ' + known if known else describe(s, m, request) or 'SAME'}")


def cmd_dump(path):
    uri, method, headers, body = decode_request(open(path, "rb").read())
    print(f"{method} {uri}")
    for k, vs in headers.items():
        for v in vs:
            print(f"{k}: {v!r}")
    print(f"\nbody ({len(body)} bytes): {body!r}")


def main(argv):
    if len(argv) < 2 or argv[1] in ("-h", "--help"):
        print(__doc__)
        return
    cmd, args = argv[1], argv[2:]
    if cmd == "seed":
        cmd_seed(args[0], args[1])
    elif cmd == "corpus-diff":
        cmd_corpus_diff(args[0], args[1])
    elif cmd == "replay":
        cmd_replay(args[0], args[1])
    elif cmd == "cluster":
        cmd_cluster(args[0], args[1], int(args[args.index("-v") + 1]) if "-v" in args else 0)
    elif cmd == "probe":
        cmd_probe(args[0], args[1], args[2])
    elif cmd == "dump":
        cmd_dump(args[0])
    else:
        sys.exit(f"unknown subcommand {cmd!r}; see --help")


if __name__ == "__main__":
    main(sys.argv)
