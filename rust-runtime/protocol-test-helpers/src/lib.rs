use http::Request;
use std::collections::HashSet;

#[derive(Debug)]
pub enum ProtocolTestFailure {
    MissingQueryParam {
        expected: String,
        found: Vec<String>,
    },
}

pub fn validate_query_string<B>(request: &Request<B>, params: &[&str]) -> Result<(), ProtocolTestFailure> {
    let query_str = request.uri().query().unwrap_or_default();
    let request_params: HashSet<&str> = query_str.split('&').collect();
    let expected: HashSet<&str> = params.iter().copied().collect();
    for param in expected {
        if !request_params.contains(param) {
            return Err(ProtocolTestFailure::MissingQueryParam {
                expected: param.to_owned(),
                found: request_params
                    .clone()
                    .into_iter()
                    .map(|x| x.to_owned())
                    .collect(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::validate_query_string;
    use http::Request;

    #[test]
    fn test_validate_query_string() {
        let request = Request::builder()
            .uri("/foo?a=b&c&d=efg&hello=a%20b")
            .body(())
            .unwrap();
        let check_pass = vec![
            vec!["a=b"],
            vec!["c", "a=b"],
            vec!["a=b", "c", "d=efg", "hello=a%20b"],
            vec![],
        ];
        for test in check_pass {
            validate_query_string(&request, test.as_slice()).expect("test should pass");
        }

        let check_fail = vec![
            vec!["a"],
            vec!["a=bc"],
            vec!["hell=a%20"],
        ];
        for test in check_fail {
            validate_query_string(&request, test.as_slice()).err().expect("test should fail");
        }
    }
}
