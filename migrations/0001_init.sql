CREATE TABLE IF NOT EXISTS migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL,
    checksum TEXT NOT NULL
);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    next_sequence INTEGER NOT NULL DEFAULT 1 CHECK (next_sequence >= 1)
);

CREATE TABLE events (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id TEXT,
    sequence INTEGER NOT NULL CHECK (sequence >= 1),
    timestamp TEXT NOT NULL,
    schema_version INTEGER NOT NULL CHECK (schema_version >= 1),
    event_json TEXT NOT NULL,
    UNIQUE (session_id, sequence)
);

CREATE INDEX events_by_session_sequence
    ON events(session_id, sequence);

CREATE TABLE messages (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id TEXT,
    role TEXT NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool')),
    content TEXT NOT NULL,
    event_sequence INTEGER,
    created_at TEXT NOT NULL,
    FOREIGN KEY (session_id, event_sequence)
        REFERENCES events(session_id, sequence)
);

CREATE INDEX messages_by_session_created_at
    ON messages(session_id, created_at);

CREATE TABLE memories (
    id TEXT PRIMARY KEY NOT NULL,
    content TEXT NOT NULL,
    provenance TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind = 'explicit'),
    state TEXT NOT NULL CHECK (state IN ('active', 'forgotten')),
    created_at TEXT NOT NULL,
    forgotten_at TEXT
);

CREATE INDEX active_memories
    ON memories(created_at)
    WHERE state = 'active';

CREATE TABLE approvals (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    tool_call_id TEXT NOT NULL,
    summary TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'allowed', 'denied', 'expired')),
    created_at TEXT NOT NULL,
    resolved_at TEXT
);

CREATE INDEX approvals_by_session_status
    ON approvals(session_id, status, created_at);

CREATE TABLE telegram_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    owner_user_id INTEGER,
    private_chat_id INTEGER,
    last_update_id INTEGER,
    updated_at TEXT NOT NULL
);

CREATE TABLE processed_telegram_updates (
    update_id INTEGER PRIMARY KEY,
    processed_at TEXT NOT NULL
);

CREATE TABLE usage_observations (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id TEXT,
    input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    observed_at TEXT NOT NULL
);

CREATE INDEX usage_by_session_observed_at
    ON usage_observations(session_id, observed_at);
