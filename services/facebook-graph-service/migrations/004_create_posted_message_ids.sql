<<<<<<< HEAD
-- Track message IDs that have been posted to Mattermost to prevent duplicates
=======
-- Migration 004: Create posted message ids table
-- Persists message IDs that have been posted to avoid duplicates

>>>>>>> dev/instagram-integration
CREATE TABLE IF NOT EXISTS posted_message_ids (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    external_id VARCHAR(255) NOT NULL,
    conversation_id VARCHAR(255) NOT NULL,
<<<<<<< HEAD
    mattermost_post_id VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(external_id)
);

CREATE INDEX IF NOT EXISTS idx_posted_message_ids_external_id ON posted_message_ids(external_id);
=======
    platform VARCHAR(50) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_posted_message_ids_external_id ON posted_message_ids(external_id);
>>>>>>> dev/instagram-integration
CREATE INDEX IF NOT EXISTS idx_posted_message_ids_conversation_id ON posted_message_ids(conversation_id);