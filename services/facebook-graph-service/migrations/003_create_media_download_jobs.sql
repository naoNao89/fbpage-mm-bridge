-- Migration 003: Create media download jobs table
-- Tracks media files that need to be downloaded from Facebook

CREATE TABLE IF NOT EXISTS media_download_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    message_external_id VARCHAR(255),
    conversation_id VARCHAR(255),
    attachment_type VARCHAR(20),
    cdn_url TEXT NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    minio_key TEXT,
    mm_file_id VARCHAR(100),
    error_message TEXT,
    retry_count INTEGER DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT media_download_jobs_status_check CHECK (status = ANY (ARRAY['pending', 'downloading', 'uploading', 'completed', 'failed']))
;

CREATE INDEX IF NOT EXISTS idx_media_jobs_conversation ON media_download_jobs(conversation_id);
CREATE INDEX IF NOT EXISTS idx_media_jobs_created ON media_download_jobs(created_at);
CREATE INDEX IF NOT EXISTS idx_media_jobs_status ON media_download_jobs(status);