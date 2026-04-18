-- Migration 003: posted_message_ids table
-- Tracks posted messages to prevent duplicates across restarts

CREATE TABLE IF NOT EXISTS posted_message_ids (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    external_id VARCHAR(255) NOT NULL,
    conversation_id VARCHAR(255) NOT NULL,
    platform VARCHAR(50) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT uq_external_id UNIQUE (external_id)
);

CREATE INDEX IF NOT EXISTS idx_posted_message_ids_external_id ON posted_message_ids(external_id);
CREATE INDEX IF NOT EXISTS idx_posted_message_ids_conversation_id ON posted_message_ids(conversation_id);