-- ============================================================
-- Sessions (rotating refresh tokens), password lifecycle, login lockout.
-- ============================================================

ALTER TABLE users ADD COLUMN IF NOT EXISTS must_change_password BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE users ADD COLUMN IF NOT EXISTS password_changed_at TIMESTAMPTZ;
ALTER TABLE users ADD COLUMN IF NOT EXISTS failed_login_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN IF NOT EXISTS locked_until TIMESTAMPTZ;
-- Embedded in every access token; bumping it invalidates all of a user's outstanding access tokens.
ALTER TABLE users ADD COLUMN IF NOT EXISTS token_version INTEGER NOT NULL DEFAULT 0;

-- Refresh tokens are opaque random strings; only their SHA-256 is stored.
-- Each login starts a "family". Every refresh rotates the token inside the family; presenting an
-- already-rotated token outside a short grace window revokes the whole family (theft detection).
CREATE TABLE IF NOT EXISTS refresh_tokens (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    user_id UUID NOT NULL,
    family_id UUID NOT NULL,
    token_hash CHAR(64) NOT NULL,
    device_label VARCHAR(100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    rotated_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    revoked_reason VARCHAR(40),
    CONSTRAINT uq_refresh_tokens_hash UNIQUE (token_hash),
    CONSTRAINT fk_refresh_tokens_user
        FOREIGN KEY (user_id, company_id) REFERENCES users(id, company_id) ON DELETE CASCADE,
    CONSTRAINT chk_refresh_tokens_revoked
        CHECK ((revoked_at IS NULL) = (revoked_reason IS NULL))
);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user ON refresh_tokens (user_id) WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_family ON refresh_tokens (family_id);
CREATE INDEX IF NOT EXISTS idx_refresh_tokens_expiry ON refresh_tokens (expires_at);

CREATE TABLE IF NOT EXISTS password_reset_tokens (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    user_id UUID NOT NULL,
    token_hash CHAR(64) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    CONSTRAINT uq_password_reset_hash UNIQUE (token_hash),
    CONSTRAINT fk_password_reset_user
        FOREIGN KEY (user_id, company_id) REFERENCES users(id, company_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_password_reset_user ON password_reset_tokens (user_id) WHERE used_at IS NULL;
