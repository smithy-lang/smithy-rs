use http::{Request, Uri};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, PartialEq, Eq, Error)]
pub enum ProtocolTestFailure {
    #[error("missing query param: expected `{expected}`, found {found:?}")]
    MissingQueryParam {
        expected: String,
        found: Vec<String>,
    },
    #[error("forbidden query param present: `{expected}`")]
    ForbiddenQueryParam {
        expected: String,
    },
    #[error("required query param missing: `{expected}`")]
    RequiredQueryParam {
        expected: String,
    },

    #[error("invalid header value for key `{key}`: expected `{expected}`, found `{found}`")]
    InvalidHeader {
        key: String,
        expected: String,
        found: String,
    },
    #[error("missing required header: `{expected}`")]
    MissingHeader {
        expected: String,
    },
}

/// Check that the protocol test succeeded & print the pretty error
/// if it did not
///
/// The primary motivation is making multiline debug output
/// readable & using the cleaner Display implementation
#[track_caller]
pub fn assert_ok(inp: Result<(), ProtocolTestFailure>) {
    match inp {
        Ok(_) => (),
        Err(e) => {
            eprintln!("{}", e);
            assert!(false, "Protocol test failed");
        }
    }
}

#[derive(Eq, PartialEq, Hash)]
struct QueryParam<'a> {
    key: &'a str,
    value: Option<&'a str>,
}

impl<'a> QueryParam<'a> {
    fn parse(s: &'a str) -> Self {
        let mut parsed = s.split('=');
        QueryParam {
            key: parsed.next().unwrap(),
            value: parsed.next(),
        }
    }
}

fn extract_params(uri: &Uri) -> HashSet<&str> {
    uri.query().unwrap_or_default().split('&').collect()
}

pub fn validate_query_string<B>(
    request: &Request<B>,
    expected_params: &[&str],
) -> Result<(), ProtocolTestFailure> {
    let actual_params = extract_params(request.uri());
    for param in expected_params {
        if !actual_params.contains(param) {
            return Err(ProtocolTestFailure::MissingQueryParam {
                expected: param.to_string(),
                found: actual_params.iter().map(|s| s.to_string()).collect(),
            });
        }
    }
    Ok(())
}

pub fn forbid_query_params<B>(
    request: &Request<B>,
    forbid_keys: &[&str],
) -> Result<(), ProtocolTestFailure> {
    let actual_keys: HashSet<&str> = extract_params(request.uri())
        .iter()
        .map(|param| QueryParam::parse(param).key)
        .collect();
    for key in forbid_keys {
        if actual_keys.contains(*key) {
            return Err(ProtocolTestFailure::ForbiddenQueryParam {
                expected: key.to_string(),
            });
        }
    }
    Ok(())
}

pub fn require_query_params<B>(
    request: &Request<B>,
    require_keys: &[&str],
) -> Result<(), ProtocolTestFailure> {
    let actual_keys: HashSet<&str> = extract_params(request.uri())
        .iter()
        .map(|param| QueryParam::parse(param).key)
        .collect();
    for key in require_keys {
        if !actual_keys.contains(*key) {
            return Err(ProtocolTestFailure::RequiredQueryParam {
                expected: key.to_string(),
            });
        }
    }
    Ok(())
}

pub fn validate_headers<B>(
    request: &Request<B>,
    expected_headers: &[(&str, &str)],
) -> Result<(), ProtocolTestFailure> {
    for (key, expected_value) in expected_headers {
        // Protocol tests store header lists as comma-delimited
        if !request.headers().contains_key(*key) {
            return Err(ProtocolTestFailure::MissingHeader {
                expected: key.to_string(),
            });
        }
        let actual_value: String = request
            .headers()
            .get_all(*key)
            .iter()
            .map(|hv| hv.to_str().unwrap())
            .collect::<Vec<_>>()
            .join(", ");
        if *expected_value != actual_value {
            return Err(ProtocolTestFailure::InvalidHeader {
                key: key.to_string(),
                expected: expected_value.to_string(),
                found: actual_value,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        forbid_query_params, require_query_params, validate_headers, validate_query_string,
        ProtocolTestFailure,
    };
    use http::Request;

    #[test]
    fn test_validate_empty_query_string() {
        let request = Request::builder().uri("/foo").body(()).unwrap();
        validate_query_string(&request, &[]).expect("no required params should pass");
        validate_query_string(&request, &["a"])
            .err()
            .expect("no params provided");
    }

    #[test]
    fn test_validate_query_string() {
        let request = Request::builder()
            .uri("/foo?a=b&c&d=efg&hello=a%20b")
            .body(())
            .unwrap();
        validate_query_string(&request, &["a=b"]).expect("a=b is in the query string");
        validate_query_string(&request, &["c", "a=b"])
            .expect("both params are in the query string");
        validate_query_string(&request, &["a=b", "c", "d=efg", "hello=a%20b"])
            .expect("all params are in the query string");
        validate_query_string(&request, &[]).expect("no required params should pass");

        validate_query_string(&request, &["a"]).expect_err("no parameter should match");
        validate_query_string(&request, &["a=bc"]).expect_err("no parameter should match");
        validate_query_string(&request, &["a=bc"]).expect_err("no parameter should match");
        validate_query_string(&request, &["hell=a%20"]).expect_err("no parameter should match");
    }

    #[test]
    fn test_forbid_query_param() {
        let request = Request::builder()
            .uri("/foo?a=b&c&d=efg&hello=a%20b")
            .body(())
            .unwrap();
        forbid_query_params(&request, &["a"]).expect_err("a is a query param");
        forbid_query_params(&request, &["not_included"]).expect("query param not included");
        forbid_query_params(&request, &["a=b"]).expect("should be matching against keys");
        forbid_query_params(&request, &["c"]).expect_err("c is a query param");
    }

    #[test]
    fn test_require_query_param() {
        let request = Request::builder()
            .uri("/foo?a=b&c&d=efg&hello=a%20b")
            .body(())
            .unwrap();
        require_query_params(&request, &["a"]).expect("a is a query param");
        require_query_params(&request, &["not_included"]).expect_err("query param not included");
        require_query_params(&request, &["a=b"]).expect_err("should be matching against keys");
        require_query_params(&request, &["c"]).expect("c is a query param");
    }

    #[test]
    fn test_validate_headers() {
        let request = Request::builder()
            .uri("/")
            .header("X-Foo", "foo")
            .header("X-Foo-List", "foo")
            .header("X-Foo-List", "bar")
            .header("X-Inline", "inline, other")
            .body(())
            .unwrap();

        validate_headers(&request, &[("X-Foo", "foo")]).expect("header present");
        validate_headers(&request, &[("X-Foo", "Foo")]).expect_err("case sensitive");
        validate_headers(&request, &[("x-foo-list", "foo, bar")]).expect("list concat");
        validate_headers(&request, &[("X-Foo-List", "foo")])
            .expect_err("all list members must be specified");
        validate_headers(&request, &[("X-Inline", "inline, other")])
            .expect("inline header lists also work");
        assert_eq!(
            validate_headers(&request, &[("missing", "value")]),
            Err(ProtocolTestFailure::MissingHeader {
                expected: "missing".to_owned()
            })
        );
    }
}
