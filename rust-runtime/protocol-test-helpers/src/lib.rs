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
    fn test_validate_empty_query_string() {
        let request = Request::builder()
            .uri("/foo")
            .body(())
            .unwrap();
        validate_query_string(&request, &vec![]).expect("no required params should pass");
        validate_query_string(&request, &vec!["a"]).err().expect("no params provided");
    }

    #[test]
    fn test_validate_query_string() {
        let request = Request::builder()
            .uri("/foo?a=b&c&d=efg&hello=a%20b")
            .body(())
            .unwrap();
        validate_query_string(&request, &vec!["a=b"]).expect("a=b is in the query string");
        validate_query_string(&request, &vec!["c", "a=b"]).expect("both params are in the query string");
        validate_query_string(&request, &vec!["a=b", "c", "d=efg", "hello=a%20b"])
            .expect("all params are in the query string");
        validate_query_string(&request, &vec![]).expect("no required params should pass");

        validate_query_string(&request, &vec!["a"]).err().expect("no parameter should match");
        validate_query_string(&request, &vec!["a=bc"]).err().expect("no parameter should match");
        validate_query_string(&request, &vec!["a=bc"]).err().expect("no parameter should match");
        validate_query_string(&request, &vec!["hell=a%20"]).err().expect("no parameter should match");

    }
}
