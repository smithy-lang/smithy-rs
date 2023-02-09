use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub struct InterceptorError {}

impl fmt::Display for InterceptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "an interceptor returned an error")
    }
}

impl std::error::Error for InterceptorError {}
