CREATE TABLE IF NOT EXISTS media_download_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    message_external_id VARCHAR(255),
    conversation_id VARCHAR(255),
    attachment_type VARCHAR(20),
    cdn_url TEXT NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'downloading', 'uploading', 'completed', 'failed')),
    minio_key TEXT,
    mm_file_id VARCHAR(100),
    error_message TEXT,
    retry_count INTEGER DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_media_jobs_status ON media_download_jobs(status);
CREATE INDEX IF NOT EXISTS idx_media_jobs_conversation ON media_download_jobs(conversation_id);
CREATE INDEX IF NOT EXISTS idx_media_jobs_created ON media_download_jobs(created_at);