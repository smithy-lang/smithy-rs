/// GenericError represents an error from a service that is not modeled
#[derive(Debug, PartialEq, Eq)]
pub struct GenericError {
    pub message: Option<String>,
    pub code: Option<String>,
    pub request_id: Option<String>,
}

impl ::std::fmt::Display for GenericError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GenericError")?;
        if let Some(inner_1) = &self.message {
            write!(f, ": {}", inner_1)?;
        }
        Ok(())
    }
}

impl ::std::error::Error for GenericError {}
