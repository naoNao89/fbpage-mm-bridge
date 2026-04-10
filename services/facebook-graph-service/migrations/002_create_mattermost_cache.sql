-- Migration: Create mattermost_cache table
-- Persists channel_id and root_post_id mappings across service restarts.

CREATE TABLE IF NOT EXISTS mattermost_cache (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    key_type VARCHAR(20) NOT NULL,        -- 'channel' or 'root'
    conversation_id VARCHAR(255) NOT NULL,
    value VARCHAR(255) NOT NULL,           -- channel_id or root_post_id
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT uq_mattermost_cache_type_conversation UNIQUE (key_type, conversation_id)
);

CREATE INDEX IF NOT EXISTS idx_mattermost_cache_key_type ON mattermost_cache(key_type);
CREATE INDEX IF NOT EXISTS idx_mattermost_cache_conversation_id ON mattermost_cache(conversation_id);