use arcwren::error::{ArcWrenError, BudgetResource, ErrorCode};
use arcwren::events::{ApprovalId, Event, EventEnvelope, EventId, SessionId, ToolCallId, TurnId};
use arcwren::runtime::budget::{BudgetTracker, TurnBudget};
use chrono::{TimeZone, Utc};
use serde_json::{Value, json};

#[test]
fn every_event_has_a_stable_type_and_schema_version() -> Result<(), Box<dyn std::error::Error>> {
    let tool_call_id = ToolCallId::new();
    let approval_id = ApprovalId::new();
    let cases = [
        (
            Event::UserInput {
                text: "hello".into(),
            },
            "user_input",
        ),
        (
            Event::AssistantTextDelta {
                text: "world".into(),
            },
            "assistant_text_delta",
        ),
        (
            Event::ToolProposed {
                tool_call_id,
                tool_name: "fs.read".into(),
                arguments: json!({"path": "notes.txt"}),
            },
            "tool_proposed",
        ),
        (
            Event::ApprovalRequested {
                approval_id,
                tool_call_id,
                summary: "Read notes.txt".into(),
            },
            "approval_requested",
        ),
        (
            Event::ToolCompleted {
                tool_call_id,
                output: json!({"text": "contents"}),
            },
            "tool_completed",
        ),
        (Event::TurnCompleted, "turn_completed"),
        (
            Event::TurnInterrupted {
                reason: "cancelled".into(),
            },
            "turn_interrupted",
        ),
    ];

    for (event, expected_type) in cases {
        let encoded = serde_json::to_value(&event)?;
        assert_eq!(encoded["type"], expected_type);
        assert_eq!(encoded["schema_version"], 1);
        assert_eq!(serde_json::from_value::<Event>(encoded)?, event);
    }

    Ok(())
}

#[test]
fn event_envelope_serializes_metadata_and_a_flattened_payload()
-> Result<(), Box<dyn std::error::Error>> {
    let id = EventId::new();
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let timestamp = Utc.with_ymd_and_hms(2026, 7, 13, 12, 34, 56).unwrap();
    let envelope = EventEnvelope {
        id,
        session_id,
        turn_id: Some(turn_id),
        sequence: 7,
        timestamp,
        event: Event::UserInput {
            text: "hello".into(),
        },
    };

    let encoded = serde_json::to_value(&envelope)?;
    assert_eq!(encoded["id"], id.to_string());
    assert_eq!(encoded["session_id"], session_id.to_string());
    assert_eq!(encoded["turn_id"], turn_id.to_string());
    assert_eq!(encoded["sequence"], 7);
    assert_eq!(encoded["schema_version"], 1);
    assert_eq!(encoded["timestamp"], "2026-07-13T12:34:56Z");
    assert_eq!(encoded["type"], "user_input");
    assert_eq!(encoded["text"], "hello");
    assert!(encoded.get("event").is_none());
    assert_eq!(envelope.schema_version(), 1);
    assert_eq!(serde_json::from_value::<EventEnvelope>(encoded)?, envelope);

    Ok(())
}

#[test]
fn ids_are_uuid_newtypes_with_string_json_representations() -> Result<(), Box<dyn std::error::Error>>
{
    let ids: [Value; 5] = [
        serde_json::to_value(SessionId::new())?,
        serde_json::to_value(TurnId::new())?,
        serde_json::to_value(EventId::new())?,
        serde_json::to_value(ToolCallId::new())?,
        serde_json::to_value(ApprovalId::new())?,
    ];

    for encoded in ids {
        let text = encoded.as_str().expect("ID must serialize as a string");
        uuid::Uuid::parse_str(text)?;
    }

    Ok(())
}

#[test]
fn budget_tracker_rejects_counts_beyond_each_limit_without_incrementing() {
    let mut tracker = BudgetTracker::new(TurnBudget {
        max_iterations: 1,
        max_tool_calls: 1,
    });

    tracker.try_record_iteration().unwrap();
    assert_eq!(tracker.iterations(), 1);
    assert_eq!(
        tracker.try_record_iteration(),
        Err(ArcWrenError::BudgetExceeded {
            resource: BudgetResource::Iterations,
            limit: 1,
        })
    );
    assert_eq!(tracker.iterations(), 1);

    tracker.try_record_tool_call().unwrap();
    assert_eq!(tracker.tool_calls(), 1);
    assert_eq!(
        tracker.try_record_tool_call(),
        Err(ArcWrenError::BudgetExceeded {
            resource: BudgetResource::ToolCalls,
            limit: 1,
        })
    );
    assert_eq!(tracker.tool_calls(), 1);
}

#[test]
fn errors_expose_stable_codes_and_sanitized_user_messages() -> Result<(), Box<dyn std::error::Error>>
{
    let secret = "provider response included sk-secret";
    let error = ArcWrenError::Provider {
        detail: secret.into(),
    };

    assert_eq!(error.code(), ErrorCode::Provider);
    assert_eq!(error.code().as_str(), "provider_error");
    assert_eq!(serde_json::to_value(error.code())?, "provider_error");
    assert!(!error.user_message().contains(secret));
    assert_eq!(
        ArcWrenError::BudgetExceeded {
            resource: BudgetResource::Iterations,
            limit: 3,
        }
        .code(),
        ErrorCode::BudgetExceeded
    );

    Ok(())
}
