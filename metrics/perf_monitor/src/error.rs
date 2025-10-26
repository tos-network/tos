use thiserror::Error;

#[derive(Error, Debug)]
pub enum MonitorError {
    #[error("Failed to collect system metrics: {0}")]
    SystemMetrics(String),

    #[error("Process not found")]
    ProcessNotFound,
}
