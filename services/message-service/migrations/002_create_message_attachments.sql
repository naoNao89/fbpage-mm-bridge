CREATE TABLE IF NOT EXISTS message_attachments (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    attachment_type VARCHAR(20) NOT NULL CHECK (attachment_type IN ('image', 'video', 'file', 'audio')),
    external_id VARCHAR(255),
    name VARCHAR(255),
    mime_type VARCHAR(100),
    size_bytes BIGINT,
    width INTEGER,
    height INTEGER,
    cdn_url TEXT,
    cdn_url_expires_at TIMESTAMPTZ,
    minio_key TEXT,
    minio_bucket VARCHAR(100),
    minio_etag VARCHAR(100),
    mm_file_id VARCHAR(100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT uq_attachment_external_id UNIQUE (external_id)
);

CREATE INDEX IF NOT EXISTS idx_message_attachments_message_id ON message_attachments(message_id);
CREATE INDEX IF NOT EXISTS idx_message_attachments_type ON message_attachments(attachment_type);
CREATE INDEX IF NOT EXISTS idx_message_attachments_cdn_expires ON message_attachments(cdn_url_expires_at) WHERE minio_key IS NULL;
CREATE INDEX IF NOT EXISTS idx_message_attachments_minio_key ON message_attachments(minio_key) WHERE minio_key IS NOT NULL;

COMMENT ON TABLE message_attachments IS 'Media attachments linked to messages, tracked from Facebook CDN to MinIO to Mattermost';