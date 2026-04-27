CREATE UNIQUE INDEX IF NOT EXISTS idx_mm_bypass_audit_op_idempotency_key
    ON mm_bypass_audit (op, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
