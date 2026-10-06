/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Opt-in comparison of fuzz results that ignores differences a client cannot observe.
//!
//! Set `SMITHY_FUZZ_SEMANTIC_COMPARE` to compare the deserialized operation input as well as the response,
//! and to treat responses as equal when they differ only in valid or omitted `content-length`, JSON member order, CBOR
//! encoding choices (definite versus indefinite lengths, integer widths, map entry order), or the order of
//! differently-named XML sibling elements.
//!
//! Set `SMITHY_FUZZ_IGNORE_UNROUTED` when the first target serves one protocol and the others serve more, to
//! accept the two differences that follow from that alone:
//! - A request the first target does not route to an operation: another target may route it through a
//!   protocol the first one does not serve.
//! - A request the first target rejects before invoking a handler and another target does not route: a
//!   multi-protocol server that finds no protocol to claim a request cannot frame the rejection the way
//!   the single protocol does.
//!
//! A request whose handler the first target invokes must still reach the same handler in every target,
//! except for the explicitly recorded X1 XML lookup without Content-Type.
//!
//! In semantic mode, F1 corrupt initial event-stream rejections, strict-claim
//! rejections and deferred CBOR routing errors are
//! classified separately as expected compatibility divergences. F1 and deferred CBOR
//! cases require rejection before either handler runs. X1 permits the recorded legacy
//! lookup handler to run; the candidate must still return the exact neutral rejection.
//! F3 permits only the recorded XML modeled event-error decoding difference with
//! unchanged successful HTTP responses; server-error responses remain failures.

use aws_smithy_fuzz::{FuzzResult, HttpRequest};
use cbor_diag::DataItem;
use std::collections::HashMap;
use std::sync::OnceLock;

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("SMITHY_FUZZ_SEMANTIC_COMPARE").is_some())
}

fn ignore_unrouted() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("SMITHY_FUZZ_IGNORE_UNROUTED").is_some())
}

/// Whether the target rejected the request in routing: no handler ran, and the status is the router's.
fn unrouted(result: &FuzzResult) -> bool {
    result.input.is_none() && matches!(result.response.status, 404 | 405)
}

/// Whether the target rejected the request as the client's fault before any handler ran.
fn rejected(result: &FuzzResult) -> bool {
    result.input.is_none() && (400..500).contains(&result.response.status)
}

/// Whether a target's result for a request agrees with the first target's result for it.
pub(crate) fn results_agree(a: &FuzzResult, b: &FuzzResult) -> bool {
    compare_results(a, b, enabled(), ignore_unrouted())
}

/// Gives the reason for a narrowly recognized intentional compatibility difference.
/// `baseline` is always the first (legacy) target, never whichever result is compared first.
pub(crate) fn expected_compatibility_divergence(
    request: &HttpRequest,
    baseline: &FuzzResult,
    candidate: &FuzzResult,
) -> Option<&'static str> {
    enabled()
        .then(|| {
            classify_initial_frame_divergence(request, baseline, candidate)
                .or_else(|| classify_xml_missing_content_type(request, baseline, candidate))
                .or_else(|| classify_xml_event_error(request, baseline, candidate))
                .or_else(|| classify_claim_divergence(request, baseline, candidate))
        })
        .flatten()
}

/// X1 is deliberately limited to the live-verified Pokemon lookup, not arbitrary lost dispatch.
fn classify_xml_missing_content_type(
    request: &HttpRequest,
    baseline: &FuzzResult,
    candidate: &FuzzResult,
) -> Option<&'static str> {
    let path = request.uri.split('?').next()?;
    let species = path.strip_prefix("/pokemon-species/")?;
    if request.method != "GET"
        || species.is_empty()
        || species.contains('/')
        || !request.body.is_empty()
        || request
            .headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("content-type"))
        || !baseline
            .input
            .as_deref()
            .is_some_and(|input| input.starts_with("GetPokemonSpeciesInput {"))
        || !matches!(baseline.response.status, 200 | 404)
        || !valid_content_length(baseline)
        || !valid_content_length(candidate)
        || headers(&baseline.response.headers).get("content-type") != Some(&"application/xml")
        || canonical_xml(&baseline.response.body).is_none()
        || candidate.input.is_some()
        || candidate.response.status != 404
        || candidate.response.body != b"<UnknownOperationException/>\n"
        || !headers(&candidate.response.headers).is_empty()
    {
        return None;
    }
    Some("X1: restXml Pokemon lookup without Content-Type; strict claiming declines the legacy request")
}

/// F3: only the recorded single XML exception and its exact handler difference.
fn classify_xml_event_error(
    request: &HttpRequest,
    baseline: &FuzzResult,
    candidate: &FuzzResult,
) -> Option<&'static str> {
    use aws_smithy_eventstream::frame::read_message_from;
    let path = request.uri.split('?').next()?;
    if request.method != "POST"
        || !(path == "/capture-pokemon-event"
            || path
                .strip_prefix("/capture-pokemon-event/")
                .is_some_and(|region| !region.is_empty() && !region.contains('/')))
        || !request.headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("content-type") && v == &["application/xml".to_owned()]
        })
        || baseline.response.status != 200
        || candidate.response.status != 200
        || !valid_content_length(baseline)
        || !valid_content_length(candidate)
        || headers(&baseline.response.headers) != headers(&candidate.response.headers)
        || !bodies_agree(&baseline.response.body, &candidate.response.body)
    {
        return None;
    }
    let mut bytes = request.body.as_slice();
    let frame = read_message_from(&mut bytes).ok()?;
    if !bytes.is_empty() || frame.headers().len() != 3 {
        return None;
    }
    for (name, value) in [
        (":message-type", "exception"),
        (":exception-type", "masterball_unsuccessful"),
        (":content-type", "application/xml"),
    ] {
        if !frame.headers().iter().any(|h| {
            h.name().as_str() == name
                && h.value()
                    .as_string()
                    .ok()
                    .is_some_and(|v| v.as_str() == value)
        }) {
            return None;
        }
    }
    let modeled = "<event-stream-service-error:MasterBallUnsuccessful(MasterBallUnsuccessful { message: Some(\"failed\") })>";
    let (left, right) = match frame.payload().as_ref() {
        b"<ErrorResponse><Error><message>failed</message></Error></ErrorResponse>" => {
            (modeled, "<event-stream-error>")
        }
        b"<MasterBallUnsuccessful><message>failed</message></MasterBallUnsuccessful>" => {
            ("<event-stream-error>", modeled)
        }
        _ => return None,
    };
    let a: Vec<String> = serde_json::from_str(baseline.input.as_deref()?).ok()?;
    let b: Vec<String> = serde_json::from_str(candidate.input.as_deref()?).ok()?;
    if a.len() < 2
        || a.first()?.as_str() != "capture_pokemon"
        || a.len() != b.len()
        || a[..a.len() - 1] != b[..b.len() - 1]
        || a.last()? != left
        || b.last()? != right
    {
        return None;
    }
    Some(
        "F3: restXml modeled event-error framing; preserve schema shape-root encoding and decoding",
    )
}

fn classify_initial_frame_divergence(
    request: &HttpRequest,
    baseline: &FuzzResult,
    candidate: &FuzzResult,
) -> Option<&'static str> {
    use aws_smithy_eventstream::frame::{DecodedFrame, MessageFrameDecoder};

    if request.method != "POST"
        || request.body.is_empty()
        || baseline.input.is_some()
        || candidate.input.is_some()
        || baseline.response.status != 400
        || candidate.response.status != 400
        || baseline.response.body != b"response error"
        || !valid_content_length(baseline)
        || !valid_content_length(candidate)
        || headers(&baseline.response.headers) != headers(&candidate.response.headers)
    {
        return None;
    }
    let response_headers = headers(&candidate.response.headers);
    if response_headers.len() != 1 {
        return None;
    }
    let content_type = *response_headers.get("content-type")?;
    let request_header = |name: &str| {
        request
            .headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .and_then(|(_, values)| (values.len() == 1).then(|| values[0].as_str()))
    };
    let expected_body: &[u8] = match content_type {
        "application/x-amz-json-1.0" | "application/x-amz-json-1.1"
            if matches!(
                request_header("content-type"),
                Some("application/vnd.amazon.eventstream")
            ) || request_header("content-type") == Some(content_type) =>
        {
            if !request_header("x-amz-target").is_some_and(|target| {
                target.ends_with(".Publish") || target.ends_with(".CapturePokemon")
            }) {
                return None;
            }
            if content_type.ends_with("1.0") {
                b"{}"
            } else {
                b""
            }
        }
        "application/cbor"
            if request_header("smithy-protocol") == Some("rpc-v2-cbor")
                && matches!(
                    request_header("content-type"),
                    Some("application/vnd.amazon.eventstream" | "application/cbor")
                )
                && (request.uri.ends_with("/operation/Publish")
                    || request.uri.ends_with("/operation/CapturePokemon")) =>
        {
            b"\xa0"
        }
        _ => return None,
    };
    if candidate.response.body != expected_body
        || matches!(
            MessageFrameDecoder::new().decode_frame(request.body.as_slice()),
            Ok(DecodedFrame::Complete(_))
        )
    {
        return None;
    }
    Some("F1: corrupt initial event-stream frame; serialization rejection replaces legacy plain-text validation error")
}

fn classify_claim_divergence(
    request: &HttpRequest,
    baseline: &FuzzResult,
    candidate: &FuzzResult,
) -> Option<&'static str> {
    if !unrouted(baseline)
        || candidate.input.is_some()
        || !valid_content_length(baseline)
        || !valid_content_length(candidate)
    {
        return None;
    }
    if candidate.response.status != 404 {
        return None;
    }
    let header = |name: &str| {
        candidate
            .response
            .headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };
    if candidate.response.body == b"<UnknownOperationException/>\n"
        && header("content-type").is_none()
        && header("smithy-protocol").is_none()
        && candidate
            .response
            .headers
            .keys()
            .all(|key| key.eq_ignore_ascii_case("content-length"))
    {
        return Some("strict claiming: no protocol identified the request; protocol-neutral 404");
    }
    let cbor_identified = request.headers.iter().any(|(name, values)| {
        name.eq_ignore_ascii_case("smithy-protocol")
            && values.len() == 1
            && values[0] == "rpc-v2-cbor"
    });
    let legacy_cbor_not_found = baseline.response.status == 404
        && baseline.response.body.is_empty()
        && baseline.response.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("content-type") && value == "application/cbor"
        });
    let legacy_method_not_allowed = baseline.response.status == 405
        && request.method != "POST"
        && baseline.response.body.is_empty()
        && baseline.response.headers.is_empty();
    if !cbor_identified
        || (!legacy_cbor_not_found && !legacy_method_not_allowed)
        || header("content-type") != Some("application/cbor")
        || header("smithy-protocol") != Some("rpc-v2-cbor")
        || !candidate.response.headers.keys().all(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                "content-type" | "smithy-protocol" | "content-length"
            )
        })
    {
        return None;
    }
    let DataItem::Map { data, .. } = cbor_diag::parse_bytes(&candidate.response.body).ok()? else {
        return None;
    };
    if data.len() != 1
        || !matches!(&data[0], (DataItem::TextString(key), DataItem::TextString(value))
        if key.data == "__type" && value.data == "smithy.framework#UnknownOperationException")
    {
        return None;
    }
    Some(if legacy_method_not_allowed {
        "deferred CBOR method rejection: configured unknown-operation 404 instead of legacy 405"
    } else {
        "deferred CBOR routing rejection: serialized unknown-operation 404 instead of legacy empty 404"
    })
}

fn compare_results(a: &FuzzResult, b: &FuzzResult, semantic: bool, ignore_routing: bool) -> bool {
    if ignore_routing && rejected(a) && (unrouted(b) || (unrouted(a) && rejected(b))) {
        return true;
    }
    if !semantic {
        return a.response == b.response;
    }
    valid_content_length(a)
        && valid_content_length(b)
        && a.input == b.input
        && a.response.status == b.response.status
        && headers(&a.response.headers) == headers(&b.response.headers)
        && bodies_agree(&a.response.body, &b.response.body)
}

// Body sizes may differ between equivalent encodings. A supplied length must
// nevertheless describe its own response; omission by a legacy server is allowed.
fn valid_content_length(result: &FuzzResult) -> bool {
    result.response.headers.iter().all(|(name, value)| {
        !name.eq_ignore_ascii_case("content-length")
            || (!value.is_empty()
                && value.bytes().all(|b| b.is_ascii_digit())
                && value.parse::<usize>().ok() == Some(result.response.body.len()))
    })
}

fn headers(headers: &HashMap<String, String>) -> HashMap<String, &str> {
    headers
        .iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("content-length"))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.as_str()))
        .collect()
}

fn bodies_agree(a: &[u8], b: &[u8]) -> bool {
    if a == b {
        return true;
    }
    if let (Ok(a), Ok(b)) = (
        serde_json::from_slice::<serde_json::Value>(a),
        serde_json::from_slice::<serde_json::Value>(b),
    ) {
        return a == b;
    }
    if let (Ok(a), Ok(b)) = (cbor_diag::parse_bytes(a), cbor_diag::parse_bytes(b)) {
        return canonical_cbor(&a) == canonical_cbor(&b);
    }
    if let (Some(a), Some(b)) = (canonical_xml(a), canonical_xml(b)) {
        return a == b;
    }
    event_streams_agree(a, b)
}

/// Compare validated frames in order, retaining all headers and raw payloads unless the
/// frame explicitly declares a structured content type. CRCs are validated by the decoder.
fn event_streams_agree(mut a: &[u8], mut b: &[u8]) -> bool {
    use aws_smithy_eventstream::frame::read_message_from;
    while !a.is_empty() && !b.is_empty() {
        let (Ok(left), Ok(right)) = (read_message_from(&mut a), read_message_from(&mut b)) else {
            return false;
        };
        let mut lh: Vec<_> = left.headers().iter().collect();
        let mut rh: Vec<_> = right.headers().iter().collect();
        lh.sort_by_key(|h| h.name().as_str());
        rh.sort_by_key(|h| h.name().as_str());
        if lh != rh {
            return false;
        }
        let content_type = lh
            .iter()
            .find(|h| h.name().as_str() == ":content-type")
            .and_then(|h| h.value().as_string().ok())
            .map(|s| s.as_str());
        let equal = match content_type {
            Some("application/json") => match (
                serde_json::from_slice::<serde_json::Value>(left.payload()),
                serde_json::from_slice::<serde_json::Value>(right.payload()),
            ) {
                (Ok(a), Ok(b)) => a == b,
                _ => left.payload() == right.payload(),
            },
            Some("application/cbor") => match (
                cbor_diag::parse_bytes(left.payload()),
                cbor_diag::parse_bytes(right.payload()),
            ) {
                (Ok(a), Ok(b)) => canonical_cbor(&a) == canonical_cbor(&b),
                _ => left.payload() == right.payload(),
            },
            Some("application/xml" | "text/xml") => match (
                canonical_xml(left.payload()),
                canonical_xml(right.payload()),
            ) {
                (Some(a), Some(b)) => a == b,
                _ => left.payload() == right.payload(),
            },
            _ => left.payload() == right.payload(),
        };
        if !equal {
            return false;
        }
    }
    a.is_empty() && b.is_empty()
}

/// Renders an XML document with every element's children stably sorted by name, so that documents
/// differing only in the order of differently-named siblings (structure members) render identically.
/// Same-named siblings (list members, map entries) keep their order.
///
/// Handles what the server serializers emit: elements, attributes and text. Returns `None` for
/// anything else, comments and mixed content included.
fn canonical_xml(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    let text = match text.strip_prefix("<?xml") {
        Some(rest) => rest.split_once("?>")?.1.trim_start(),
        None => text,
    };
    let (_, rendered, rest) = xml_element(text)?;
    rest.trim().is_empty().then_some(rendered)
}

/// Parses one element at the start of `text`, returning its name, canonical rendering and the remainder.
fn xml_element(text: &str) -> Option<(&str, String, &str)> {
    let text = text.strip_prefix('<')?;
    let tag_end = text.find('>')?;
    let (tag, mut rest) = (&text[..tag_end], &text[tag_end + 1..]);
    if tag.starts_with(['!', '?', '/']) {
        return None;
    }
    let (tag, self_closing) = match tag.strip_suffix('/') {
        Some(tag) => (tag, true),
        None => (tag, false),
    };
    let name = tag.split_whitespace().next()?;
    let attributes = tag[name.len()..].trim();
    let mut children: Vec<(&str, String)> = Vec::new();
    let mut content = "";
    if !self_closing {
        loop {
            let next = rest.find('<')?;
            let leading = &rest[..next];
            if let Some(after) = rest[next..].strip_prefix("</") {
                let after = after.strip_prefix(name)?.trim_start().strip_prefix('>')?;
                if children.is_empty() {
                    content = leading;
                } else if !leading.trim().is_empty() {
                    return None;
                }
                rest = after;
                break;
            }
            if !leading.trim().is_empty() {
                return None;
            }
            let (child, rendered, after) = xml_element(&rest[next..])?;
            children.push((child, rendered));
            rest = after;
        }
    }
    children.sort_by(|a, b| a.0.cmp(b.0));
    let children: String = children.into_iter().map(|(_, rendered)| rendered).collect();
    Some((
        name,
        format!("<{name} {attributes}>{content}{children}</{name}>"),
        rest,
    ))
}

/// Renders a CBOR item so that encodings of the same value render identically.
fn canonical_cbor(item: &DataItem) -> String {
    match item {
        DataItem::Integer { value, .. } => value.to_string(),
        DataItem::Negative { value, .. } => format!("-{}", *value as u128 + 1),
        DataItem::ByteString(bytes) => format!("h{:?}", bytes.data),
        DataItem::IndefiniteByteString(chunks) => {
            format!(
                "h{:?}",
                chunks
                    .iter()
                    .flat_map(|c| c.data.iter().copied())
                    .collect::<Vec<_>>()
            )
        }
        DataItem::TextString(text) => format!("{:?}", text.data),
        DataItem::IndefiniteTextString(chunks) => {
            format!(
                "{:?}",
                chunks.iter().map(|c| c.data.as_str()).collect::<String>()
            )
        }
        DataItem::Array { data, .. } => {
            format!(
                "[{}]",
                data.iter()
                    .map(canonical_cbor)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        DataItem::Map { data, .. } => {
            let mut entries: Vec<_> = data
                .iter()
                .map(|(k, v)| format!("{}:{}", canonical_cbor(k), canonical_cbor(v)))
                .collect();
            entries.sort();
            format!("{{{}}}", entries.join(","))
        }
        DataItem::Tag { tag, value, .. } => format!("{}({})", tag.0, canonical_cbor(value)),
        DataItem::Float { value, .. } => format!("{value:?}"),
        DataItem::Simple(simple) => format!("simple({})", simple.0),
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_xml;

    #[test]
    fn strict_claim_divergences_are_narrow_and_never_hide_handler_regressions() {
        use aws_smithy_fuzz::{FuzzResult, HttpRequest, HttpResponse};
        use std::collections::HashMap;
        let request = HttpRequest {
            method: "GET".into(),
            uri: "/service/Service/operation/unknown".into(),
            headers: HashMap::from([("smithy-protocol".into(), vec!["rpc-v2-cbor".into()])]),
            ..Default::default()
        };
        let baseline = FuzzResult {
            response: HttpResponse {
                status: 405,
                ..Default::default()
            },
            input: None,
        };
        let neutral = FuzzResult {
            response: HttpResponse {
                status: 404,
                body: b"<UnknownOperationException/>\n".to_vec(),
                ..Default::default()
            },
            input: None,
        };
        let classify =
            |a: &FuzzResult, b: &FuzzResult| super::classify_claim_divergence(&request, a, b);
        assert_eq!(
            classify(&baseline, &neutral),
            Some("strict claiming: no protocol identified the request; protocol-neutral 404")
        );
        let mut body = vec![0xbf, 0x66];
        body.extend(b"__type");
        body.extend([0x78, 0x2a]);
        body.extend(b"smithy.framework#UnknownOperationException");
        body.push(0xff);
        let cbor = FuzzResult {
            response: HttpResponse {
                status: 404,
                headers: HashMap::from([
                    ("content-type".into(), "application/cbor".into()),
                    ("smithy-protocol".into(), "rpc-v2-cbor".into()),
                ]),
                body,
            },
            input: None,
        };
        assert_eq!(classify(&baseline, &cbor), Some(
            "deferred CBOR method rejection: configured unknown-operation 404 instead of legacy 405"
        ));
        let mut not_found = baseline.clone();
        not_found.response.status = 404;
        not_found
            .response
            .headers
            .insert("content-type".into(), "application/cbor".into());
        assert_eq!(classify(&not_found, &cbor), Some(
            "deferred CBOR routing rejection: serialized unknown-operation 404 instead of legacy empty 404"
        ));
        for expected in [&neutral, &cbor] {
            let mut accepted = baseline.clone();
            accepted.input = Some("handler ran".into());
            accepted.response.status = 200;
            assert_eq!(classify(&accepted, expected), None);
            // Historical routing suppression must not hide lost handler dispatch either.
            assert!(!super::compare_results(&accepted, expected, true, true));
            for status in [200, 400, 403, 405, 500] {
                let mut wrong = expected.clone();
                wrong.response.status = status;
                assert_eq!(classify(&baseline, &wrong), None, "{status}");
            }
            let mut wrong = expected.clone();
            wrong.input = Some("handler ran".into());
            assert_eq!(classify(&baseline, &wrong), None);
            let mut wrong = expected.clone();
            wrong
                .response
                .headers
                .insert("content-length".into(), "999".into());
            assert_eq!(classify(&baseline, &wrong), None);
            let mut wrong = expected.clone();
            wrong.response.body.push(0);
            assert_eq!(classify(&baseline, &wrong), None);
        }
        let mut unidentified = request.clone();
        unidentified.headers.clear();
        assert_eq!(
            super::classify_claim_divergence(&unidentified, &baseline, &cbor),
            None
        );
        let mut post = request.clone();
        post.method = "POST".into();
        assert_eq!(
            super::classify_claim_divergence(&post, &baseline, &cbor),
            None
        );
        assert_eq!(
            classify(&neutral, &baseline),
            None,
            "classification is directional"
        );
    }

    #[test]
    fn xml_claim_divergence_contract() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/xml-claim-divergences.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let request = serde_json::from_value(case["request"].clone()).unwrap();
            let baseline = serde_json::from_value(case["baseline"].clone()).unwrap();
            let candidate = serde_json::from_value(case["candidate"].clone()).unwrap();
            assert_eq!(
                super::classify_xml_missing_content_type(&request, &baseline, &candidate).is_some(),
                case["known"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn xml_event_error_divergence_contract() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/xml-event-error-divergences.json"))
                .unwrap();
        for case in cases.as_array().unwrap() {
            let request = serde_json::from_value(case["request"].clone()).unwrap();
            let baseline = serde_json::from_value(case["baseline"].clone()).unwrap();
            let candidate = serde_json::from_value(case["candidate"].clone()).unwrap();
            assert_eq!(
                super::classify_xml_event_error(&request, &baseline, &candidate).is_some(),
                case["known"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn known_divergence_contract() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/known-divergences.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let request = serde_json::from_value(case["request"].clone()).unwrap();
            let baseline = serde_json::from_value(case["baseline"].clone()).unwrap();
            let candidate = serde_json::from_value(case["candidate"].clone()).unwrap();
            assert_eq!(
                super::classify_initial_frame_divergence(&request, &baseline, &candidate).is_some(),
                case["known"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn compatibility_contract() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/compatibility.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let a = serde_json::from_value(case["a"].clone()).unwrap();
            let b = serde_json::from_value(case["b"].clone()).unwrap();
            assert_eq!(
                super::compare_results(&a, &b, true, false),
                case["agree"].as_bool().unwrap(),
                "{}",
                case["name"]
            );
            assert_eq!(
                super::compare_results(&a, &b, false, false),
                a.response == b.response
            );
        }
    }

    fn frame(name: &str, content_type: &str, payload: &[u8]) -> Vec<u8> {
        use aws_smithy_types::event_stream::{Header, HeaderValue, Message};
        let message = Message::new_from_parts(
            vec![
                Header::new(":message-type", HeaderValue::String("event".into())),
                Header::new(":event-type", HeaderValue::String(name.to_owned().into())),
                Header::new(
                    ":content-type",
                    HeaderValue::String(content_type.to_owned().into()),
                ),
            ],
            payload.to_vec(),
        );
        let mut bytes = Vec::new();
        aws_smithy_eventstream::frame::write_message_to(&message, &mut bytes).unwrap();
        bytes
    }

    #[test]
    fn event_stream_payloads_are_compared_by_declared_content_type() {
        for (ct, a, b) in [
            (
                "application/json",
                br#"{"a":1,"b":2}"#.as_slice(),
                br#"{"b":2,"a":1}"#.as_slice(),
            ),
            (
                "application/cbor",
                b"\xa1\x61a\x01".as_slice(),
                b"\xbf\x61a\x18\x01\xff".as_slice(),
            ),
            (
                "application/xml",
                b"<E><a>1</a><b>2</b></E>".as_slice(),
                b"<E><b>2</b><a>1</a></E>".as_slice(),
            ),
        ] {
            assert!(super::bodies_agree(
                &frame("note", ct, a),
                &frame("note", ct, b)
            ));
            assert!(!super::bodies_agree(
                &frame("note", "application/octet-stream", a),
                &frame("note", "application/octet-stream", b)
            ));
        }
    }

    #[test]
    fn event_stream_order_headers_checksums_and_truncation_matter() {
        let a = frame("first", "application/json", b"{}");
        let b = frame("second", "application/json", b"{}");
        assert!(!super::bodies_agree(&a, &b));
        assert!(!super::bodies_agree(
            &[a.clone(), b.clone()].concat(),
            &[b.clone(), a.clone()].concat()
        ));
        assert!(!super::bodies_agree(&a, &[a.clone(), b].concat()));
        assert!(!super::bodies_agree(&a, &a[..a.len() - 1]));
        let mut bad = a.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert!(!super::bodies_agree(&a, &bad));
    }

    #[test]
    fn xml_sibling_order_is_ignored_for_different_names_only() {
        let a = canonical_xml(b"<Out><b>1</b><a><member>x</member><member>y</member></a></Out>");
        let b = canonical_xml(b"<?xml version=\"1.0\"?><Out><a><member>x</member><member>y</member></a><b>1</b></Out>\n");
        let reordered_list =
            canonical_xml(b"<Out><a><member>y</member><member>x</member></a><b>1</b></Out>");
        assert!(a.is_some());
        assert_eq!(a, b);
        assert_ne!(a, reordered_list);
        assert_eq!(canonical_xml(b"<a/>"), canonical_xml(b"<a></a>"));
        assert_ne!(canonical_xml(b"<a x=\"1\"/>"), canonical_xml(b"<a/>"));
    }

    #[test]
    fn xml_that_is_not_one_plain_element_is_not_canonicalized() {
        for text in [
            "",
            "{}",
            "<a>",
            "<a></b>",
            "<a/><b/>",
            "<a>x<b/></a>",
            "<!-- c --><a/>",
        ] {
            assert_eq!(canonical_xml(text.as_bytes()), None, "{text}");
        }
    }
}
