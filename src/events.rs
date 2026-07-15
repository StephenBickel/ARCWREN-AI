use chrono::{DateTime, Utc};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

pub const EVENT_SCHEMA_VERSION: u32 = 1;

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self::from_uuid(value)
            }
        }

        impl From<$name> for Uuid {
            fn from(value: $name) -> Self {
                value.as_uuid()
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self::from_uuid)
            }
        }
    };
}

define_id!(SessionId);
define_id!(TurnId);
define_id!(EventId);
define_id!(ToolCallId);
define_id!(ApprovalId);

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    UserInput {
        text: String,
    },
    AssistantTextDelta {
        text: String,
    },
    ToolProposed {
        tool_call_id: ToolCallId,
        tool_name: String,
        arguments: Value,
    },
    ApprovalRequested {
        approval_id: ApprovalId,
        tool_call_id: ToolCallId,
        summary: String,
    },
    ToolCompleted {
        tool_call_id: ToolCallId,
        output: Value,
    },
    TurnCompleted,
    TurnInterrupted {
        reason: String,
    },
}

impl Event {
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        EVENT_SCHEMA_VERSION
    }
}

#[derive(Serialize)]
struct VersionedEventRef<'a> {
    schema_version: u32,
    #[serde(flatten)]
    payload: EventRef<'a>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EventRef<'a> {
    UserInput {
        text: &'a str,
    },
    AssistantTextDelta {
        text: &'a str,
    },
    ToolProposed {
        tool_call_id: ToolCallId,
        tool_name: &'a str,
        arguments: &'a Value,
    },
    ApprovalRequested {
        approval_id: ApprovalId,
        tool_call_id: ToolCallId,
        summary: &'a str,
    },
    ToolCompleted {
        tool_call_id: ToolCallId,
        output: &'a Value,
    },
    TurnCompleted,
    TurnInterrupted {
        reason: &'a str,
    },
}

impl<'a> From<&'a Event> for EventRef<'a> {
    fn from(event: &'a Event) -> Self {
        match event {
            Event::UserInput { text } => Self::UserInput { text },
            Event::AssistantTextDelta { text } => Self::AssistantTextDelta { text },
            Event::ToolProposed {
                tool_call_id,
                tool_name,
                arguments,
            } => Self::ToolProposed {
                tool_call_id: *tool_call_id,
                tool_name,
                arguments,
            },
            Event::ApprovalRequested {
                approval_id,
                tool_call_id,
                summary,
            } => Self::ApprovalRequested {
                approval_id: *approval_id,
                tool_call_id: *tool_call_id,
                summary,
            },
            Event::ToolCompleted {
                tool_call_id,
                output,
            } => Self::ToolCompleted {
                tool_call_id: *tool_call_id,
                output,
            },
            Event::TurnCompleted => Self::TurnCompleted,
            Event::TurnInterrupted { reason } => Self::TurnInterrupted { reason },
        }
    }
}

impl Serialize for Event {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        VersionedEventRef {
            schema_version: EVENT_SCHEMA_VERSION,
            payload: EventRef::from(self),
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
struct VersionedEvent {
    schema_version: u32,
    #[serde(flatten)]
    payload: EventPayload,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EventPayload {
    UserInput {
        text: String,
    },
    AssistantTextDelta {
        text: String,
    },
    ToolProposed {
        tool_call_id: ToolCallId,
        tool_name: String,
        arguments: Value,
    },
    ApprovalRequested {
        approval_id: ApprovalId,
        tool_call_id: ToolCallId,
        summary: String,
    },
    ToolCompleted {
        tool_call_id: ToolCallId,
        output: Value,
    },
    TurnCompleted,
    TurnInterrupted {
        reason: String,
    },
}

impl From<EventPayload> for Event {
    fn from(payload: EventPayload) -> Self {
        match payload {
            EventPayload::UserInput { text } => Self::UserInput { text },
            EventPayload::AssistantTextDelta { text } => Self::AssistantTextDelta { text },
            EventPayload::ToolProposed {
                tool_call_id,
                tool_name,
                arguments,
            } => Self::ToolProposed {
                tool_call_id,
                tool_name,
                arguments,
            },
            EventPayload::ApprovalRequested {
                approval_id,
                tool_call_id,
                summary,
            } => Self::ApprovalRequested {
                approval_id,
                tool_call_id,
                summary,
            },
            EventPayload::ToolCompleted {
                tool_call_id,
                output,
            } => Self::ToolCompleted {
                tool_call_id,
                output,
            },
            EventPayload::TurnCompleted => Self::TurnCompleted,
            EventPayload::TurnInterrupted { reason } => Self::TurnInterrupted { reason },
        }
    }
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let event = VersionedEvent::deserialize(deserializer)?;
        if event.schema_version != EVENT_SCHEMA_VERSION {
            return Err(D::Error::custom(format_args!(
                "unsupported event schema version {}",
                event.schema_version
            )));
        }
        Ok(event.payload.into())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EventEnvelope {
    pub id: EventId,
    pub session_id: SessionId,
    pub turn_id: Option<TurnId>,
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub event: Event,
}

impl EventEnvelope {
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.event.schema_version()
    }
}
