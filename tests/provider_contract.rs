use std::error::Error;

use arcwren::events::ToolCallId;
use arcwren::providers::scripted::ScriptedProvider;
use arcwren::providers::{
    FinishReason, Message, MessageContent, ModelRequest, ModelSettings, Provider,
    ProviderCapabilities, ProviderError, ProviderEvent, Role, ToolDefinition,
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

const TOOL_CALL_ID: &str = "11111111-1111-4111-8111-111111111111";
const FIXTURE: &str = include_str!("fixtures/provider/tool_then_answer.json");

fn tool_call_id() -> ToolCallId {
    TOOL_CALL_ID.parse().expect("fixture tool-call ID is valid")
}

fn request(cancellation: CancellationToken) -> ModelRequest {
    ModelRequest {
        messages: vec![
            Message {
                role: Role::System,
                content: vec![MessageContent::Text {
                    text: "Be concise.".into(),
                }],
            },
            Message {
                role: Role::User,
                content: vec![MessageContent::Text {
                    text: "Read README.md".into(),
                }],
            },
        ],
        tools: vec![ToolDefinition {
            name: "fs.read".into(),
            description: "Read a bounded UTF-8 file".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
                "additionalProperties": false,
            }),
        }],
        settings: ModelSettings {
            model: "fixture-model".into(),
            temperature: Some(0.2),
            max_output_tokens: Some(512),
        },
        cancellation,
    }
}

#[test]
fn normalized_request_has_an_exact_provider_neutral_wire_contract() -> Result<(), Box<dyn Error>> {
    let encoded = serde_json::to_value(request(CancellationToken::new()))?;
    assert_eq!(
        encoded,
        json!({
            "messages": [
                {
                    "role": "system",
                    "content": [{"type": "text", "text": "Be concise."}],
                },
                {
                    "role": "user",
                    "content": [{"type": "text", "text": "Read README.md"}],
                },
            ],
            "tools": [{
                "name": "fs.read",
                "description": "Read a bounded UTF-8 file",
                "input_schema": {
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"],
                    "additionalProperties": false,
                },
            }],
            "settings": {
                "model": "fixture-model",
                "temperature": 0.2,
                "max_output_tokens": 512,
            },
        })
    );
    assert!(encoded.get("cancellation").is_none());

    let decoded: ModelRequest = serde_json::from_value(encoded)?;
    assert_eq!(decoded.messages, request(CancellationToken::new()).messages);
    assert_eq!(decoded.tools, request(CancellationToken::new()).tools);
    assert_eq!(decoded.settings, request(CancellationToken::new()).settings);
    assert!(!decoded.cancellation.is_cancelled());
    Ok(())
}

#[test]
fn normalized_message_content_carries_tool_calls_and_results() -> Result<(), Box<dyn Error>> {
    let messages = vec![
        Message {
            role: Role::Assistant,
            content: vec![MessageContent::ToolCall {
                tool_call_id: tool_call_id(),
                name: "fs.read".into(),
                arguments: json!({"path": "README.md"}),
            }],
        },
        Message {
            role: Role::Tool,
            content: vec![MessageContent::ToolResult {
                tool_call_id: tool_call_id(),
                output: json!({"text": "# ArcWren"}),
            }],
        },
    ];
    let expected = json!([
        {
            "role": "assistant",
            "content": [{
                "type": "tool_call",
                "tool_call_id": TOOL_CALL_ID,
                "name": "fs.read",
                "arguments": {"path": "README.md"},
            }],
        },
        {
            "role": "tool",
            "content": [{
                "type": "tool_result",
                "tool_call_id": TOOL_CALL_ID,
                "output": {"text": "# ArcWren"},
            }],
        },
    ]);

    assert_eq!(serde_json::to_value(&messages)?, expected);
    assert_eq!(serde_json::from_value::<Vec<Message>>(expected)?, messages);
    Ok(())
}

#[test]
fn capabilities_and_events_have_exact_serializable_contracts() -> Result<(), Box<dyn Error>> {
    let capabilities = ProviderCapabilities {
        streaming: true,
        structured_tool_calls: true,
        parallel_tool_calls: false,
        usage_reporting: true,
        context_window: Some(128_000),
    };
    let encoded_capabilities = json!({
        "streaming": true,
        "structured_tool_calls": true,
        "parallel_tool_calls": false,
        "usage_reporting": true,
        "context_window": 128000,
    });
    assert_eq!(serde_json::to_value(capabilities)?, encoded_capabilities);
    assert_eq!(
        serde_json::from_value::<ProviderCapabilities>(encoded_capabilities)?,
        capabilities
    );

    let cases = [
        (
            ProviderEvent::TextDelta {
                text: "The file says ".into(),
            },
            json!({"type": "text_delta", "text": "The file says "}),
        ),
        (
            ProviderEvent::ToolCall {
                tool_call_id: tool_call_id(),
                name: "fs.read".into(),
                arguments: json!({"path": "README.md"}),
            },
            json!({
                "type": "tool_call",
                "tool_call_id": TOOL_CALL_ID,
                "name": "fs.read",
                "arguments": {"path": "README.md"},
            }),
        ),
        (
            ProviderEvent::Usage {
                input_tokens: 48,
                output_tokens: 9,
            },
            json!({"type": "usage", "input_tokens": 48, "output_tokens": 9}),
        ),
        (
            ProviderEvent::Finish {
                reason: FinishReason::ToolCalls,
            },
            json!({"type": "finish", "reason": "tool_calls"}),
        ),
    ];

    for (event, expected) in cases {
        assert_eq!(serde_json::to_value(&event)?, expected);
        assert_eq!(serde_json::from_value::<ProviderEvent>(expected)?, event);
    }
    Ok(())
}

#[tokio::test]
async fn scripted_provider_replays_complete_responses_and_records_complete_requests()
-> Result<(), Box<dyn Error>> {
    let provider = ScriptedProvider::from_json(FIXTURE)?;
    assert_eq!(
        provider.capabilities(),
        ProviderCapabilities {
            streaming: true,
            structured_tool_calls: true,
            parallel_tool_calls: false,
            usage_reporting: true,
            context_window: Some(128_000),
        }
    );

    let first_cancellation = CancellationToken::new();
    let first_request = request(first_cancellation.clone());
    let first = provider
        .stream(first_request.clone())
        .await?
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        first,
        vec![
            ProviderEvent::ToolCall {
                tool_call_id: tool_call_id(),
                name: "fs.read".into(),
                arguments: json!({"path": "README.md"}),
            },
            ProviderEvent::Usage {
                input_tokens: 48,
                output_tokens: 9,
            },
            ProviderEvent::Finish {
                reason: FinishReason::ToolCalls,
            },
        ]
    );

    let second_request = request(CancellationToken::new());
    let second = provider
        .stream(second_request.clone())
        .await?
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        second,
        vec![
            ProviderEvent::TextDelta {
                text: "The file says ".into(),
            },
            ProviderEvent::TextDelta {
                text: "# ArcWren".into(),
            },
            ProviderEvent::Usage {
                input_tokens: 71,
                output_tokens: 14,
            },
            ProviderEvent::Finish {
                reason: FinishReason::Stop,
            },
        ]
    );

    let recorded = provider.recorded_requests();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].messages, first_request.messages);
    assert_eq!(recorded[0].tools, first_request.tools);
    assert_eq!(recorded[0].settings, first_request.settings);
    first_cancellation.cancel();
    assert!(recorded[0].cancellation.is_cancelled());
    assert_eq!(recorded[1].messages, second_request.messages);
    assert_eq!(recorded[1].tools, second_request.tools);
    assert_eq!(recorded[1].settings, second_request.settings);
    Ok(())
}

#[tokio::test]
async fn scripted_stream_observes_cancellation_between_events() -> Result<(), Box<dyn Error>> {
    let provider = ScriptedProvider::from_json(FIXTURE)?;
    let cancellation = CancellationToken::new();
    let mut stream = provider.stream(request(cancellation.clone())).await?;

    assert!(matches!(
        stream.next().await,
        Some(Ok(ProviderEvent::ToolCall { .. }))
    ));
    cancellation.cancel();
    assert_eq!(stream.next().await, Some(Err(ProviderError::Cancelled)));
    assert_eq!(stream.next().await, None);
    Ok(())
}

#[tokio::test]
async fn scripted_provider_reports_cancellation_before_stream_creation() {
    let provider = ScriptedProvider::from_json(FIXTURE).unwrap();
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    assert!(matches!(
        provider.stream(request(cancellation)).await,
        Err(ProviderError::Cancelled)
    ));
    assert!(provider.recorded_requests().is_empty());
}

#[tokio::test]
async fn scripted_provider_reports_fixture_exhaustion_without_reusing_a_response()
-> Result<(), Box<dyn Error>> {
    let provider = ScriptedProvider::from_json(FIXTURE)?;
    for _ in 0..2 {
        provider
            .stream(request(CancellationToken::new()))
            .await?
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
    }

    assert!(matches!(
        provider.stream(request(CancellationToken::new())).await,
        Err(ProviderError::ScriptExhausted { response_count: 2 })
    ));
    assert_eq!(provider.recorded_requests().len(), 2);
    Ok(())
}

#[test]
fn malformed_scripted_fixtures_are_rejected_with_typed_errors() {
    let cases = [
        ("not JSON", "valid JSON"),
        (
            r#"{"schema_version":2,"capabilities":{"streaming":true,"structured_tool_calls":false,"parallel_tool_calls":false,"usage_reporting":false,"context_window":null},"responses":[]}"#,
            "schema version 2",
        ),
        (
            r#"{"schema_version":1,"capabilities":{"streaming":true,"structured_tool_calls":false,"parallel_tool_calls":false,"usage_reporting":false,"context_window":null},"responses":[{"events":[{"type":"text_delta","text":"unfinished"}]}]}"#,
            "finish event",
        ),
        (
            r#"{"schema_version":1,"capabilities":{"streaming":true,"structured_tool_calls":false,"parallel_tool_calls":false,"usage_reporting":false,"context_window":null},"responses":[{"events":[{"type":"finish","reason":"stop"},{"type":"text_delta","text":"late"}]}]}"#,
            "final event",
        ),
        (
            r#"{"schema_version":1,"capabilities":{"streaming":true,"structured_tool_calls":false,"parallel_tool_calls":false,"usage_reporting":false,"context_window":null},"responses":[{"events":[{"type":"tool_call","tool_call_id":"11111111-1111-4111-8111-111111111111","name":"fs.read","arguments":{"path":"README.md"}},{"type":"finish","reason":"tool_calls"}]}]}"#,
            "structured_tool_calls",
        ),
        (
            r#"{"schema_version":1,"capabilities":{"streaming":true,"structured_tool_calls":true,"parallel_tool_calls":false,"usage_reporting":false,"context_window":null},"responses":[{"events":[{"type":"tool_call","tool_call_id":"11111111-1111-4111-8111-111111111111","name":"fs.read","arguments":"README.md"},{"type":"finish","reason":"tool_calls"}]}]}"#,
            "arguments must be an object",
        ),
    ];

    for (fixture, expected_detail) in cases {
        assert!(matches!(
            ScriptedProvider::from_json(fixture),
            Err(ProviderError::InvalidFixture { ref detail })
                if detail.contains(expected_detail),
        ));
    }
}

#[tokio::test]
async fn provider_trait_is_usable_as_a_dynamic_async_boundary() -> Result<(), Box<dyn Error>> {
    async fn first_event(provider: &dyn Provider) -> Result<ProviderEvent, ProviderError> {
        provider
            .stream(request(CancellationToken::new()))
            .await?
            .next()
            .await
            .expect("fixture response is non-empty")
    }

    let provider = ScriptedProvider::from_json(FIXTURE)?;
    assert!(matches!(
        first_event(&provider).await?,
        ProviderEvent::ToolCall { .. }
    ));
    Ok(())
}

#[test]
fn fixture_is_complete_json_with_no_provider_specific_wire_fields() -> Result<(), Box<dyn Error>> {
    let fixture: Value = serde_json::from_str(FIXTURE)?;
    assert_eq!(fixture["schema_version"], 1);
    assert_eq!(fixture["responses"].as_array().map(Vec::len), Some(2));
    assert!(FIXTURE.contains("tool_call"));
    assert!(FIXTURE.contains("text_delta"));
    assert!(FIXTURE.contains("usage"));
    assert!(FIXTURE.contains("finish"));
    for provider_specific in ["choices", "response.output_item", "data:"] {
        assert!(!FIXTURE.contains(provider_specific));
    }
    Ok(())
}
