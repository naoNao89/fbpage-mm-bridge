-- Migration 003: Create media download jobs table
-- Tracks media files that need to be downloaded from Facebook

CREATE TABLE IF NOT EXISTS media_download_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    message_id VARCHAR(255) NOT NULL,
    media_url VARCHAR(1024) NOT NULL,
    media_type VARCHAR(50) NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    retry_count INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    CONSTRAINT uq_media_download_jobs_message_media UNIQUE (message_id, media_url)
);

CREATE INDEX IF NOT EXISTS idx_media_download_jobs_status ON media_download_jobs(status);
CREATE INDEX IF NOT EXISTS idx_media_download_jobs_created_at ON media_download_jobs(created_at DESC);