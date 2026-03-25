-- Migration: Create facebook_rate_limits table
-- This table tracks Facebook Graph API rate limits

CREATE TABLE IF NOT EXISTS facebook_rate_limits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    endpoint VARCHAR(100) NOT NULL UNIQUE,
    calls_remaining INTEGER,
    calls_total INTEGER,
    reset_at TIMESTAMPTZ,
    last_response_headers JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for quick lookups
CREATE INDEX IF NOT EXISTS idx_facebook_rate_limits_endpoint ON facebook_rate_limits(endpoint);

-- Index for reset_at to find expired limits
CREATE INDEX IF NOT EXISTS idx_facebook_rate_limits_reset_at ON facebook_rate_limits(reset_at);

-- Import job tracking table
CREATE TABLE IF NOT EXISTS facebook_import_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    total_conversations INTEGER DEFAULT 0,
    processed_conversations INTEGER DEFAULT 0,
    failed_conversations INTEGER DEFAULT 0,
    total_messages INTEGER DEFAULT 0,
    messages_stored INTEGER DEFAULT 0,
    messages_skipped INTEGER DEFAULT 0,
    error_message TEXT,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Conversation import tracking
CREATE TABLE IF NOT EXISTS facebook_conversation_imports (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    job_id UUID REFERENCES facebook_import_jobs(id) ON DELETE SET NULL,
    conversation_id VARCHAR(255) NOT NULL UNIQUE,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    messages_fetched INTEGER DEFAULT 0,
    messages_stored INTEGER DEFAULT 0,
    error_message TEXT,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_fb_conversation_imports_job_id ON facebook_conversation_imports(job_id);
CREATE INDEX IF NOT EXISTS idx_fb_conversation_imports_conversation_id ON facebook_conversation_imports(conversation_id);
CREATE INDEX IF NOT EXISTS idx_fb_conversation_imports_status ON facebook_conversation_imports(status);
