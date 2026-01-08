use metrique::Slot;
use metrique_macro::metrics;

#[metrics]
#[derive(Default)]
pub struct DefaultMetrics {
    #[metrics(flatten)]
    pub(crate) request_metrics: Option<Slot<DefaultRequestMetrics>>,
    #[metrics(flatten)]
    pub(crate) response_metrics: Option<DefaultResponseMetrics>,
}

#[metrics]
#[derive(Default)]
pub struct DefaultRequestMetrics {
    pub(crate) request_id: Option<String>,
}

#[metrics]
#[derive(Default)]
pub struct DefaultResponseMetrics {
    pub(crate) http_status_code: Option<u16>,
}
