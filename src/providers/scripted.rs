use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Mutex, MutexGuard};
use std::task::{Context, Poll};

use futures_core::Stream;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::{
    FinishReason, ModelRequest, Provider, ProviderCapabilities, ProviderError, ProviderEvent,
    ProviderFuture, ProviderStream,
};

const SCRIPT_SCHEMA_VERSION: u32 = 1;

pub struct ScriptedProvider {
    capabilities: ProviderCapabilities,
    response_count: usize,
    state: Mutex<ScriptState>,
}

struct ScriptState {
    responses: VecDeque<Vec<ProviderEvent>>,
    recorded_requests: Vec<ModelRequest>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptFixture {
    schema_version: u32,
    capabilities: ProviderCapabilities,
    responses: Vec<ScriptResponse>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptResponse {
    events: Vec<ProviderEvent>,
}

impl ScriptedProvider {
    pub fn from_json(json: &str) -> Result<Self, ProviderError> {
        let fixture: ScriptFixture =
            serde_json::from_str(json).map_err(|error| ProviderError::InvalidFixture {
                detail: format!("fixture must be valid JSON: {error}"),
            })?;
        validate_fixture(&fixture)?;

        let responses = fixture
            .responses
            .into_iter()
            .map(|response| response.events)
            .collect::<VecDeque<_>>();
        let response_count = responses.len();
        Ok(Self {
            capabilities: fixture.capabilities,
            response_count,
            state: Mutex::new(ScriptState {
                responses,
                recorded_requests: Vec::new(),
            }),
        })
    }

    #[must_use]
    pub fn recorded_requests(&self) -> Vec<ModelRequest> {
        lock_state(&self.state)
            .recorded_requests
            .iter()
            .map(detached_request)
            .collect()
    }
}

impl Provider for ScriptedProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities
    }

    fn stream(&self, request: ModelRequest) -> ProviderFuture<'_> {
        Box::pin(async move {
            if request.cancellation.is_cancelled() {
                return Err(ProviderError::Cancelled);
            }

            let cancellation = request.cancellation.clone();
            let events = {
                let mut state = lock_state(&self.state);
                let events = state
                    .responses
                    .pop_front()
                    .ok_or(ProviderError::ScriptExhausted {
                        response_count: self.response_count,
                    })?;
                state.recorded_requests.push(detached_request(&request));
                events
            };

            Ok(Box::pin(ScriptedEventStream {
                events: events.into(),
                cancellation,
                finished: false,
            }) as ProviderStream)
        })
    }
}

struct ScriptedEventStream {
    events: VecDeque<ProviderEvent>,
    cancellation: CancellationToken,
    finished: bool,
}

impl Stream for ScriptedEventStream {
    type Item = Result<ProviderEvent, ProviderError>;

    fn poll_next(mut self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }
        if self.cancellation.is_cancelled() {
            self.finished = true;
            return Poll::Ready(Some(Err(ProviderError::Cancelled)));
        }

        match self.events.pop_front() {
            Some(event) => {
                if matches!(event, ProviderEvent::Finish { .. }) {
                    self.finished = true;
                }
                Poll::Ready(Some(Ok(event)))
            }
            None => {
                self.finished = true;
                Poll::Ready(None)
            }
        }
    }
}

fn validate_fixture(fixture: &ScriptFixture) -> Result<(), ProviderError> {
    if fixture.schema_version != SCRIPT_SCHEMA_VERSION {
        return invalid_fixture(format!(
            "unsupported scripted provider schema version {}",
            fixture.schema_version
        ));
    }
    if !fixture.capabilities.streaming {
        return invalid_fixture("scripted provider fixtures require streaming capability");
    }
    if fixture.capabilities.parallel_tool_calls && !fixture.capabilities.structured_tool_calls {
        return invalid_fixture(
            "parallel_tool_calls requires the structured_tool_calls capability",
        );
    }
    if fixture.responses.is_empty() {
        return invalid_fixture("scripted provider fixtures require at least one response");
    }

    for (response_index, response) in fixture.responses.iter().enumerate() {
        let response_number = response_index + 1;
        if response.events.is_empty() {
            return invalid_fixture(format!("response {response_number} has no events"));
        }
        if !matches!(response.events.last(), Some(ProviderEvent::Finish { .. })) {
            return invalid_fixture(format!(
                "response {response_number} must end with a finish event as its final event"
            ));
        }

        let mut finish_count = 0;
        let mut tool_call_count = 0;
        for event in &response.events {
            match event {
                ProviderEvent::TextDelta { text } if text.is_empty() => {
                    return invalid_fixture(format!(
                        "response {response_number} contains an empty text delta"
                    ));
                }
                ProviderEvent::ToolCall {
                    name, arguments, ..
                } => {
                    tool_call_count += 1;
                    if !fixture.capabilities.structured_tool_calls {
                        return invalid_fixture(format!(
                            "response {response_number} emits a tool call but structured_tool_calls is false"
                        ));
                    }
                    if name.trim().is_empty() {
                        return invalid_fixture(format!(
                            "response {response_number} contains an empty tool name"
                        ));
                    }
                    if !arguments.is_object() {
                        return invalid_fixture(format!(
                            "response {response_number} tool arguments must be an object"
                        ));
                    }
                    if tool_call_count > 1 && !fixture.capabilities.parallel_tool_calls {
                        return invalid_fixture(format!(
                            "response {response_number} contains multiple tool calls but parallel_tool_calls is false"
                        ));
                    }
                }
                ProviderEvent::Usage { .. } if !fixture.capabilities.usage_reporting => {
                    return invalid_fixture(format!(
                        "response {response_number} emits usage but usage_reporting is false"
                    ));
                }
                ProviderEvent::Finish { reason } => {
                    finish_count += 1;
                    if *reason == FinishReason::ToolCalls && tool_call_count == 0 {
                        return invalid_fixture(format!(
                            "response {response_number} finishes for tool_calls without a tool call"
                        ));
                    }
                }
                ProviderEvent::TextDelta { .. } | ProviderEvent::Usage { .. } => {}
            }
        }
        if finish_count != 1 {
            return invalid_fixture(format!(
                "response {response_number} must contain exactly one finish event"
            ));
        }
    }
    Ok(())
}

fn invalid_fixture<T>(detail: impl Into<String>) -> Result<T, ProviderError> {
    Err(ProviderError::InvalidFixture {
        detail: detail.into(),
    })
}

fn detached_request(request: &ModelRequest) -> ModelRequest {
    ModelRequest {
        messages: request.messages.clone(),
        tools: request.tools.clone(),
        settings: request.settings.clone(),
        cancellation: CancellationToken::new(),
    }
}

fn lock_state(state: &Mutex<ScriptState>) -> MutexGuard<'_, ScriptState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
