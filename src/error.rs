use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ErrorCode {
    #[serde(rename = "configuration_error")]
    Configuration,
    #[serde(rename = "authentication_error")]
    Authentication,
    #[serde(rename = "provider_error")]
    Provider,
    #[serde(rename = "rate_limit")]
    RateLimit,
    #[serde(rename = "policy_error")]
    Policy,
    #[serde(rename = "validation_error")]
    Validation,
    #[serde(rename = "tool_error")]
    Tool,
    #[serde(rename = "storage_error")]
    Storage,
    #[serde(rename = "channel_error")]
    Channel,
    #[serde(rename = "timeout")]
    Timeout,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "budget_exceeded")]
    BudgetExceeded,
}

impl ErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configuration => "configuration_error",
            Self::Authentication => "authentication_error",
            Self::Provider => "provider_error",
            Self::RateLimit => "rate_limit",
            Self::Policy => "policy_error",
            Self::Validation => "validation_error",
            Self::Tool => "tool_error",
            Self::Storage => "storage_error",
            Self::Channel => "channel_error",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::BudgetExceeded => "budget_exceeded",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetResource {
    Iterations,
    ToolCalls,
}

impl fmt::Display for BudgetResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Iterations => formatter.write_str("iterations"),
            Self::ToolCalls => formatter.write_str("tool calls"),
        }
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ArcWrenError {
    #[error("configuration error: {detail}")]
    Configuration { detail: String },
    #[error("authentication error: {detail}")]
    Authentication { detail: String },
    #[error("provider error: {detail}")]
    Provider { detail: String },
    #[error("provider rate limit: {detail}")]
    RateLimit { detail: String },
    #[error("policy error: {detail}")]
    Policy { detail: String },
    #[error("validation error: {detail}")]
    Validation { detail: String },
    #[error("tool error: {detail}")]
    Tool { detail: String },
    #[error("storage error: {detail}")]
    Storage { detail: String },
    #[error("channel error: {detail}")]
    Channel { detail: String },
    #[error("operation timed out: {detail}")]
    Timeout { detail: String },
    #[error("operation cancelled: {detail}")]
    Cancelled { detail: String },
    #[error("turn budget exceeded for {resource} (limit: {limit})")]
    BudgetExceeded {
        resource: BudgetResource,
        limit: u32,
    },
}

impl ArcWrenError {
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Configuration { .. } => ErrorCode::Configuration,
            Self::Authentication { .. } => ErrorCode::Authentication,
            Self::Provider { .. } => ErrorCode::Provider,
            Self::RateLimit { .. } => ErrorCode::RateLimit,
            Self::Policy { .. } => ErrorCode::Policy,
            Self::Validation { .. } => ErrorCode::Validation,
            Self::Tool { .. } => ErrorCode::Tool,
            Self::Storage { .. } => ErrorCode::Storage,
            Self::Channel { .. } => ErrorCode::Channel,
            Self::Timeout { .. } => ErrorCode::Timeout,
            Self::Cancelled { .. } => ErrorCode::Cancelled,
            Self::BudgetExceeded { .. } => ErrorCode::BudgetExceeded,
        }
    }

    #[must_use]
    pub const fn user_message(&self) -> &'static str {
        match self {
            Self::Configuration { .. } => "ArcWren's configuration is invalid.",
            Self::Authentication { .. } => "Authentication failed.",
            Self::Provider { .. } => "The model provider request failed.",
            Self::RateLimit { .. } => "The model provider is temporarily rate limited.",
            Self::Policy { .. } => "The requested action is not allowed.",
            Self::Validation { .. } => "The request is invalid.",
            Self::Tool { .. } => "The tool failed.",
            Self::Storage { .. } => "ArcWren could not access its local data.",
            Self::Channel { .. } => "The frontend connection failed.",
            Self::Timeout { .. } => "The operation timed out.",
            Self::Cancelled { .. } => "The operation was cancelled.",
            Self::BudgetExceeded { .. } => "The turn reached its configured budget.",
        }
    }
}
