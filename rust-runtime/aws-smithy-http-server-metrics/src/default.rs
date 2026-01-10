use metrique::Slot;
use metrique::SlotGuard;
use metrique_macro::metrics;

#[metrics]
#[derive(Default)]
pub struct DefaultMetrics {
    #[metrics(flatten)]
    pub(crate) request_metrics: Option<Slot<DefaultRequestMetrics>>,
    #[metrics(flatten)]
    pub(crate) response_metrics: Option<Slot<DefaultResponseMetrics>>,
}

#[metrics]
#[derive(Default)]
pub struct DefaultRequestMetrics {
    pub(crate) service_name: Option<String>,
    pub(crate) service_version: Option<String>,
    pub(crate) operation_name: Option<String>,
    pub(crate) request_id: Option<String>,
}

#[metrics]
#[derive(Default)]
pub struct DefaultResponseMetrics {
    pub(crate) http_status_code: Option<u16>,
}

#[derive(Default, Clone)]
pub(crate) struct DefaultRequestMetricsConfig {
    pub(crate) disable_all: bool,
    pub(crate) disable_request_id: bool,
    pub(crate) disable_operation_name: bool,
    pub(crate) disable_service_name: bool,
    pub(crate) disable_service_version: bool,
}

#[derive(Default, Clone)]
pub(crate) struct DefaultResponseMetricsConfig {
    pub(crate) disable_all: bool,
    pub(crate) disable_http_status_code: bool,
}

pub(crate) struct DefaultRequestMetricsExtension {
    pub(crate) metrics: SlotGuard<DefaultRequestMetrics>,
    pub(crate) config: DefaultRequestMetricsConfig,
}

pub(crate) struct DefaultResponseMetricsExtension {
    pub(crate) metrics: SlotGuard<DefaultResponseMetrics>,
    pub(crate) config: DefaultResponseMetricsConfig,
}
