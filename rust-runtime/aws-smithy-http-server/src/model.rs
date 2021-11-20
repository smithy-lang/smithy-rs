pub struct HealthcheckInput;
pub struct HealthcheckOutput;
pub(crate) struct HealthcheckOperationInput(pub HealthcheckInput);
pub(crate) struct HealthcheckOperationOutput(pub HealthcheckOutput);

pub struct RegisterServiceInput;
pub struct RegisterServiceOutput;
pub struct RegisterServiceError;
pub(crate) struct RegisterServiceOperationInput(pub RegisterServiceInput);

pub enum RegisterServiceOperationOutput {
    Output(RegisterServiceOutput),
    Error(RegisterServiceError),
}

impl From<HealthcheckOperationInput> for HealthcheckInput {
    fn from(v: HealthcheckOperationInput) -> Self {
        v.0
    }
}

impl From<HealthcheckOutput> for HealthcheckOperationOutput {
    fn from(v: HealthcheckOutput) -> Self {
        HealthcheckOperationOutput(v)
    }
}

impl From<RegisterServiceOperationInput> for RegisterServiceInput {
    fn from(v: RegisterServiceOperationInput) -> Self {
        v.0
    }
}

impl From<Result<RegisterServiceOutput, RegisterServiceError>> for RegisterServiceOperationOutput {
    fn from(res: Result<RegisterServiceOutput, RegisterServiceError>) -> Self {
        match res {
            Ok(v) => RegisterServiceOperationOutput::Output(v),
            Err(e) => RegisterServiceOperationOutput::Error(e),
        }
    }
}
