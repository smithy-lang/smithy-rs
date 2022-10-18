use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum PyMiddlewareError {
    ResponseAlreadyGone,
}

impl fmt::Display for PyMiddlewareError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::ResponseAlreadyGone => write!(f, "response is already consumed"),
        }
    }
}

impl Error for PyMiddlewareError {}
