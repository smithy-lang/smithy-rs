/// GenericError represents an error from a service that is not modeled
#[derive(Debug, PartialEq, Eq, ::serde::Deserialize)]
pub struct GenericError {
    message: Option<String>,
    #[serde(alias = "__type")]
    code: Option<String>,
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
