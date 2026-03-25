-- Create messages table for Message Service
-- This table stores messages from various platforms (Facebook, Zalo, etc.)

CREATE TABLE IF NOT EXISTS messages (
    id UUID PRIMARY KEY,
    customer_id UUID NOT NULL,
    conversation_id VARCHAR(255) NOT NULL,
    platform VARCHAR(50) NOT NULL,
    direction VARCHAR(20) NOT NULL CHECK (direction IN ('incoming', 'outgoing')),
    message_text TEXT,
    external_id VARCHAR(255),
    mattermost_channel VARCHAR(255),
    mattermost_synced_at TIMESTAMP WITH TIME ZONE,
    mattermost_sync_error TEXT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Ensure unique external_id per platform (if provided)
    CONSTRAINT unique_external_id UNIQUE (platform, external_id)
);

-- Index for efficient lookups by customer
CREATE INDEX IF NOT EXISTS idx_messages_customer_id ON messages(customer_id);

-- Index for efficient lookups by conversation
CREATE INDEX IF NOT EXISTS idx_messages_conversation_id ON messages(conversation_id);

-- Index for efficient lookups by external_id
CREATE INDEX IF NOT EXISTS idx_messages_external_id ON messages(external_id);

-- Index for finding unsynced messages (for Mattermost sync)
CREATE INDEX IF NOT EXISTS idx_messages_unsynced ON messages(mattermost_synced_at, mattermost_sync_error);

-- Index for time-based queries
CREATE INDEX IF NOT EXISTS idx_messages_created_at ON messages(created_at DESC);

-- Index for platform filtering
CREATE INDEX IF NOT EXISTS idx_messages_platform ON messages(platform);

-- Comment on table
COMMENT ON TABLE messages IS 'Messages from various platforms with Mattermost sync tracking';
COMMENT ON COLUMN messages.id IS 'Unique identifier for the message';
COMMENT ON COLUMN messages.customer_id IS 'Foreign key to the customer who sent/received this message';
COMMENT ON COLUMN messages.conversation_id IS 'Platform-specific conversation identifier (e.g., FB conversation ID)';
COMMENT ON COLUMN messages.platform IS 'Platform identifier (facebook, zalo)';
COMMENT ON COLUMN messages.direction IS 'Message direction: incoming (customer to page) or outgoing (page to customer)';
COMMENT ON COLUMN messages.message_text IS 'The actual message content';
COMMENT ON COLUMN messages.external_id IS 'Platform-specific message ID for deduplication';
COMMENT ON COLUMN messages.mattermost_channel IS 'Mattermost channel this message was synced to';
COMMENT ON COLUMN messages.mattermost_synced_at IS 'Timestamp when message was successfully synced to Mattermost';
COMMENT ON COLUMN messages.mattermost_sync_error IS 'Error message if sync to Mattermost failed';
COMMENT ON COLUMN messages.created_at IS 'Timestamp when message was received/posted on the platform';
