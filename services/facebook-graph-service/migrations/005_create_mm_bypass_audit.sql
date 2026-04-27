-- Migration 005: Audit table for Mattermost direct-DB-bypass operations
--
-- Every call routed through `MattermostOps` (the bypass façade) writes one
-- row here regardless of whether the API or DB path was taken. Used both
-- for compliance (the bypass skips Mattermost's own audit log) and for
-- idempotency on operations that accept an Idempotency-Key header.

CREATE TABLE IF NOT EXISTS mm_bypass_audit (
    id BIGSERIAL PRIMARY KEY,
    op TEXT NOT NULL,
    params_hash TEXT NOT NULL,
    path_taken TEXT NOT NULL,
    fallback_reason TEXT,
    status TEXT NOT NULL,
    idempotency_key TEXT,
    result_id TEXT,
    duration_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_mm_bypass_audit_op_created
    ON mm_bypass_audit (op, created_at DESC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mm_bypass_audit_idem
    ON mm_bypass_audit (op, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
