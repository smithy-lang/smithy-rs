//! Scenarios replayed against every implementation of the review services (mainline, branch legacy
//! twin, schemaSerde). Shared by `protocol-compat` and the `protocol-compat-mainline` recorder.
use super::*;
use http::Request;
use serde_json::json;

pub type Build = fn() -> Request<Body>;

pub fn snapshot(reply: &Reply) -> String {
    let mut headers: Vec<String> = reply
        .headers
        .iter()
        .filter(|(name, _)| name.as_str() != "date" && !name.as_str().contains("request-id") && !name.as_str().contains("requestid"))
        .map(|(name, value)| format!("{}: {}", name, value.to_str().unwrap_or("<binary>")))
        .collect();
    headers.sort();
    let is_cbor = reply.header("content-type").is_some_and(|c| c.contains("cbor"));
    let body = if is_cbor && !reply.body.is_empty() {
        std::panic::catch_unwind(|| reply.cbor().to_string()).unwrap_or_else(|_| format!("{:?}", reply.body))
    } else if let Ok(parsed) = serde_json::from_slice::<serde_json::Value>(&reply.body) {
        // Canonical form: map member order is not part of the contract.
        parsed.to_string()
    } else {
        reply.text()
    };
    let encoding = if is_cbor { format!(" encoding={}", reply.body.iter().map(|b| format!("{b:02x}")).collect::<String>()) } else { String::new() };
    format!("{} [{}] {}{}", reply.status, headers.join("; "), body, encoding)
}

const JSON: &[(&str, &str)] = &[("content-type", "application/json")];

pub fn rest_scenarios() -> Vec<(&'static str, Build)> {
    vec![
        ("get label without content-type", || rest("GET", "/widgets/123", &[], b"")),
        ("get label with json content-type", || rest("GET", "/widgets/123", JSON, b"")),
        ("delete label without content-type", || rest("DELETE", "/widgets/123", &[], b"")),
        ("post no-input without content-type", || rest("POST", "/ping", &[], b"")),
        ("post no-input with json content-type", || rest("POST", "/ping", JSON, b"")),
        ("post no-input with xml content-type", || rest("POST", "/ping", &[("content-type", "application/xml")], b"")),
        ("post no-input with body", || rest("POST", "/ping", JSON, b"{}")),
        ("wrong method", || rest("PUT", "/widgets/123", &[], b"")),
        ("unknown path", || rest("GET", "/no/such/operation", JSON, b"")),
        ("root path", || rest("GET", "/", &[], b"")),
        ("label percent-encoded", || rest("GET", "/widgets/a%20b", &[], b"")),
        ("label empty", || rest("GET", "/widgets/", &[], b"")),
        ("trailing slash label", || rest("GET", "/pets/rex", &[], b"")),
        ("trailing slash label with slash", || rest("GET", "/pets/rex/", &[], b"")),
        ("greedy label", || rest("GET", "/data/a/b", &[], b"")),
        ("greedy label with slash", || rest("GET", "/data/a/b/", &[], b"")),
        ("response @http code", || rest("POST", "/response-code", &[], b"")),
        ("response @httpResponseCode member", || rest("POST", "/response-code-member", &[], b"")),
        ("response @httpHeader", || rest("POST", "/response-header", &[], b"")),
        ("struct payload", || rest("POST", "/struct-payload", &[("content-type", "application/json"), ("x-h", "hv")], br#"{"a":"x"}"#)),
        ("struct payload invalid json", || rest("POST", "/struct-payload", JSON, b"{not json")),
        ("struct payload wrong content-type", || rest("POST", "/struct-payload", &[("content-type", "text/plain")], br#"{"a":"x"}"#)),
        ("struct payload empty body", || rest("POST", "/struct-payload", JSON, b"")),
        ("struct payload null body", || rest("POST", "/struct-payload", JSON, b"null")),
        ("streaming blob output", || rest("GET", "/download", &[], b"")),
        ("prefix headers trimmed", || rest("POST", "/prefix", &[("x-meta-a", "  padded \t"), ("x-meta-b", "plain")], b"")),
        ("prefix headers empty value", || rest("POST", "/prefix", &[("x-meta-a", "")], b"")),
        ("mediaType map base64", || rest("POST", "/media", &[("x-json-a", "dHJ1ZQ==")], b"")),
        ("mediaType map invalid base64", || rest("POST", "/media", &[("x-json-a", "!!")], b"")),
        ("non-json-number bigdecimal", || rest("POST", "/sensitive", &[], b"")),
        ("unformattable sensitive timestamp", || rest("POST", "/sensitive-timestamp", &[], b"")),
    ]
}

pub fn aws_json_scenarios() -> Vec<(&'static str, Build)> {
    vec![
        ("get widget", || aws_json("1.0", "ReviewAwsJson", "GetWidget", r#"{"id":"123"}"#)),
        ("get widget missing required", || aws_json("1.0", "ReviewAwsJson", "GetWidget", "{}")),
        ("ping", || aws_json("1.0", "ReviewAwsJson", "Ping", "{}")),
        ("ping empty body", || aws_json("1.0", "ReviewAwsJson", "Ping", "")),
        ("response @http code", || aws_json("1.0", "ReviewAwsJson", "ResponseCode", "{}")),
        ("response @httpResponseCode member", || aws_json("1.0", "ReviewAwsJson", "ResponseCodeMember", "{}")),
        ("response @httpHeader", || aws_json("1.0", "ReviewAwsJson", "ResponseHeader", "{}")),
        ("struct payload", || aws_json("1.0", "ReviewAwsJson", "StructPayload", r#"{"h":"hv","payload":{"a":"x"}}"#)),
        ("struct payload invalid json", || aws_json("1.0", "ReviewAwsJson", "StructPayload", "{not json")),
        ("streaming blob output", || aws_json("1.0", "ReviewAwsJson", "DownloadBlob", "{}")),
        ("prefix headers (body member)", || aws_json("1.0", "ReviewAwsJson", "PrefixHeaders", "{}")),
        ("non-json-number bigdecimal", || aws_json("1.0", "ReviewAwsJson", "SensitiveAmount", "{}")),
        ("unformattable sensitive timestamp", || aws_json("1.0", "ReviewAwsJson", "SensitiveTimestamp", "{}")),
        ("unknown target", || aws_json("1.0", "ReviewAwsJson", "NoSuchOperation", "{}")),
        ("wrong service in target", || aws_json("1.0", "Other", "Ping", "{}")),
        ("wrong content-type version", || aws_json("1.1", "ReviewAwsJson", "Ping", "{}")),
        ("no target header", || rest("POST", "/", &[("content-type", "application/x-amz-json-1.0")], b"{}")),
        ("GET root", || rest("GET", "/", &[], b"")),
        ("rest-style path", || rest("GET", "/widgets/123", &[], b"")),
    ]
}

pub fn cbor_scenarios() -> Vec<(&'static str, Build)> {
    vec![
        ("get widget", || cbor("ReviewCbor", "GetWidget", &json!({"id": "123"}))),
        ("get widget missing required", || cbor("ReviewCbor", "GetWidget", &json!({}))),
        ("ping", || cbor("ReviewCbor", "Ping", &json!({}))),
        ("ping empty body", || {
            Request::builder()
                .method("POST")
                .uri("/service/ReviewCbor/operation/Ping")
                .header("smithy-protocol", "rpc-v2-cbor")
                .header("accept", "application/cbor")
                .body(body(b""))
                .unwrap()
        }),
        ("response @http code", || cbor("ReviewCbor", "ResponseCode", &json!({}))),
        ("response @httpResponseCode member", || cbor("ReviewCbor", "ResponseCodeMember", &json!({}))),
        ("response @httpHeader", || cbor("ReviewCbor", "ResponseHeader", &json!({}))),
        ("struct payload", || cbor("ReviewCbor", "StructPayload", &json!({"h": "hv", "payload": {"a": "x"}}))),
        ("streaming blob output", || cbor("ReviewCbor", "DownloadBlob", &json!({}))),
        ("non-json-number bigdecimal", || cbor("ReviewCbor", "SensitiveAmount", &json!({}))),
        ("unformattable sensitive timestamp", || cbor("ReviewCbor", "SensitiveTimestamp", &json!({}))),
        ("unknown operation", || cbor("ReviewCbor", "NoSuchOperation", &json!({}))),
        ("unknown service", || cbor("Other", "Ping", &json!({}))),
        ("missing smithy-protocol header", || {
            Request::builder()
                .method("POST")
                .uri("/service/ReviewCbor/operation/Ping")
                .header("content-type", "application/cbor")
                .body(body(&[0xa0]))
                .unwrap()
        }),
        ("wrong smithy-protocol header", || {
            Request::builder()
                .method("POST")
                .uri("/service/ReviewCbor/operation/Ping")
                .header("content-type", "application/cbor")
                .header("smithy-protocol", "nope")
                .body(body(&[0xa0]))
                .unwrap()
        }),
        ("x-amz-target header present", || {
            Request::builder()
                .method("POST")
                .uri("/service/ReviewCbor/operation/Ping")
                .header("content-type", "application/cbor")
                .header("smithy-protocol", "rpc-v2-cbor")
                .header("x-amz-target", "ReviewCbor.Ping")
                .body(body(&[0xa0]))
                .unwrap()
        }),
        ("wrong content-type", || {
            Request::builder()
                .method("POST")
                .uri("/service/ReviewCbor/operation/Ping")
                .header("content-type", "application/json")
                .header("smithy-protocol", "rpc-v2-cbor")
                .body(body(&[0xa0]))
                .unwrap()
        }),
        ("malformed cbor", || {
            Request::builder()
                .method("POST")
                .uri("/service/ReviewCbor/operation/GetWidget")
                .header("content-type", "application/cbor")
                .header("smithy-protocol", "rpc-v2-cbor")
                .body(body(&[0xff, 0x00]))
                .unwrap()
        }),
        ("GET on operation path", || {
            Request::builder()
                .method("GET")
                .uri("/service/ReviewCbor/operation/Ping")
                .header("smithy-protocol", "rpc-v2-cbor")
                .body(body(b""))
                .unwrap()
        }),
        ("rest-style path", || rest("GET", "/widgets/123", &[], b"")),
    ]
}

pub fn document_scenarios() -> Vec<(&'static str, Build)> {
    vec![
        ("null document", || rest("POST", "/null-document", JSON, b"null")),
        ("object document", || rest("POST", "/null-document", JSON, br#"{"a":1}"#)),
        ("empty body", || rest("POST", "/null-document", JSON, b"")),
        ("invalid json", || rest("POST", "/null-document", JSON, b"{x")),
        ("number document", || rest("POST", "/null-document", JSON, b"42")),
    ]
}

