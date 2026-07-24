use std::fmt;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use url::Url;

const MAX_AUTHORIZATION_URL_BYTES: usize = 8_192;
const MAX_USER_CODE_BYTES: usize = 128;

pub type AuthFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, AuthError>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SubscriptionService {
    #[serde(rename = "openai_codex")]
    OpenAiCodex,
    #[serde(rename = "xai_grok")]
    XaiGrok,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuthMethod {
    #[serde(rename = "browser_oauth")]
    BrowserOAuth,
    #[serde(rename = "device_code")]
    DeviceCode,
    #[serde(rename = "provider_managed")]
    ProviderManaged,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SubscriptionPlan {
    #[serde(rename = "free")]
    Free,
    #[serde(rename = "go")]
    Go,
    #[serde(rename = "plus")]
    Plus,
    #[serde(rename = "pro")]
    Pro,
    #[serde(rename = "pro_lite")]
    ProLite,
    #[serde(rename = "team")]
    Team,
    #[serde(rename = "business")]
    Business,
    #[serde(rename = "enterprise")]
    Enterprise,
    #[serde(rename = "education")]
    Education,
    #[serde(rename = "super_grok")]
    SuperGrok,
    #[serde(rename = "x_premium")]
    XPremium,
    #[serde(rename = "x_premium_plus")]
    XPremiumPlus,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuthUnavailableCode {
    #[serde(rename = "executable_missing")]
    ExecutableMissing,
    #[serde(rename = "unsupported_version")]
    UnsupportedVersion,
    #[serde(rename = "keyring_unavailable")]
    KeyringUnavailable,
    #[serde(rename = "protocol_mismatch")]
    ProtocolMismatch,
    #[serde(rename = "provider_rejected")]
    ProviderRejected,
    #[serde(rename = "timed_out")]
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthState {
    SignedOut,
    Pending,
    SignedIn {
        method: AuthMethod,
        plan: Option<SubscriptionPlan>,
    },
    Unavailable {
        code: AuthUnavailableCode,
    },
}

#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum AuthStateWire {
    SignedOut {},
    Pending {},
    SignedIn {
        method: AuthMethod,
        plan: Option<SubscriptionPlan>,
    },
    Unavailable {
        code: AuthUnavailableCode,
    },
}

impl<'de> Deserialize<'de> for AuthState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match AuthStateWire::deserialize(deserializer)? {
            AuthStateWire::SignedOut {} => Ok(Self::SignedOut),
            AuthStateWire::Pending {} => Ok(Self::Pending),
            AuthStateWire::SignedIn { method, plan } => Ok(Self::SignedIn { method, plan }),
            AuthStateWire::Unavailable { code } => Ok(Self::Unavailable { code }),
        }
    }
}

#[derive(Eq, PartialEq)]
pub struct AuthorizationUrl(Url);

impl AuthorizationUrl {
    pub fn parse(value: &str) -> Result<Self, AuthError> {
        if value.is_empty()
            || value.len() > MAX_AUTHORIZATION_URL_BYTES
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err(AuthError::from_code(AuthErrorCode::InvalidAuthorizationUrl));
        }

        let url = Url::parse(value)
            .map_err(|_| AuthError::from_code(AuthErrorCode::InvalidAuthorizationUrl))?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(AuthError::from_code(AuthErrorCode::InvalidAuthorizationUrl));
        }

        Ok(Self(url))
    }

    #[must_use]
    pub fn into_foreground_string(self) -> String {
        self.0.to_string()
    }
}

impl fmt::Debug for AuthorizationUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthorizationUrl(<redacted>)")
    }
}

#[derive(Eq, PartialEq)]
pub struct UserCode(String);

impl UserCode {
    pub fn parse(value: &str) -> Result<Self, AuthError> {
        if value.is_empty()
            || value.len() > MAX_USER_CODE_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(AuthError::from_code(AuthErrorCode::InvalidUserCode));
        }

        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn into_foreground_string(self) -> String {
        self.0
    }
}

impl fmt::Debug for UserCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UserCode(<redacted>)")
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum LoginChallenge {
    Browser {
        authorization_url: AuthorizationUrl,
    },
    Device {
        verification_url: AuthorizationUrl,
        user_code: UserCode,
    },
}

pub trait SubscriptionAuthBroker: Send {
    fn service(&self) -> SubscriptionService;

    fn auth_state(&mut self) -> AuthFuture<'_, AuthState>;

    fn start_login(&mut self, method: AuthMethod) -> AuthFuture<'_, LoginChallenge>;

    fn logout(&mut self) -> AuthFuture<'_, ()>;

    fn cancel_login(&mut self) -> AuthFuture<'_, ()>;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuthErrorCode {
    #[serde(rename = "invalid_authorization_url")]
    InvalidAuthorizationUrl,
    #[serde(rename = "invalid_user_code")]
    InvalidUserCode,
    #[serde(rename = "executable_missing")]
    ExecutableMissing,
    #[serde(rename = "unsupported_version")]
    UnsupportedVersion,
    #[serde(rename = "keyring_unavailable")]
    KeyringUnavailable,
    #[serde(rename = "protocol_mismatch")]
    ProtocolMismatch,
    #[serde(rename = "provider_rejected")]
    ProviderRejected,
    #[serde(rename = "timed_out")]
    TimedOut,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "sidecar_exited")]
    SidecarExited,
}

impl AuthErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidAuthorizationUrl => "invalid_authorization_url",
            Self::InvalidUserCode => "invalid_user_code",
            Self::ExecutableMissing => "executable_missing",
            Self::UnsupportedVersion => "unsupported_version",
            Self::KeyringUnavailable => "keyring_unavailable",
            Self::ProtocolMismatch => "protocol_mismatch",
            Self::ProviderRejected => "provider_rejected",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::SidecarExited => "sidecar_exited",
        }
    }
}

impl fmt::Display for AuthErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Error, PartialEq, Serialize)]
#[error("subscription authentication failed: {code}")]
#[serde(transparent)]
pub struct AuthError {
    code: AuthErrorCode,
}

impl AuthError {
    #[must_use]
    pub const fn from_code(code: AuthErrorCode) -> Self {
        Self { code }
    }

    #[must_use]
    pub const fn code(&self) -> AuthErrorCode {
        self.code
    }
}

impl From<AuthUnavailableCode> for AuthErrorCode {
    fn from(code: AuthUnavailableCode) -> Self {
        match code {
            AuthUnavailableCode::ExecutableMissing => Self::ExecutableMissing,
            AuthUnavailableCode::UnsupportedVersion => Self::UnsupportedVersion,
            AuthUnavailableCode::KeyringUnavailable => Self::KeyringUnavailable,
            AuthUnavailableCode::ProtocolMismatch => Self::ProtocolMismatch,
            AuthUnavailableCode::ProviderRejected => Self::ProviderRejected,
            AuthUnavailableCode::TimedOut => Self::TimedOut,
        }
    }
}

impl From<AuthUnavailableCode> for AuthError {
    fn from(code: AuthUnavailableCode) -> Self {
        Self::from_code(code.into())
    }
}
