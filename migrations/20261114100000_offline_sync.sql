-- ============================================================
-- Offline synchronisation
--  1. Trigger-maintained updated_at on every table offline clients pull
--     (so the delta feed cannot miss a write, whatever code path made it).
--  2. Keyset indexes for the delta feed.
--  3. sync_operations: journal of operations uploaded by offline devices
--     (idempotent by client_op_id, keeps rejected operations for review).
--  4. sync.use / sync.manage permissions.
-- ============================================================

-- 1. updated_at maintenance ------------------------------------------------
CREATE OR REPLACE FUNCTION sync_touch_updated_at()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at := clock_timestamp();
    RETURN NEW;
END;
$$;

DO $$
DECLARE t TEXT;
BEGIN
    FOREACH t IN ARRAY ARRAY[
        'categories','products','product_variants','customers',
        'stock_balances','inventory_locations','cash_registers'
    ] LOOP
        EXECUTE format('DROP TRIGGER IF EXISTS trg_sync_touch ON %I', t);
        EXECUTE format(
            'CREATE TRIGGER trg_sync_touch BEFORE INSERT OR UPDATE ON %I
             FOR EACH ROW EXECUTE FUNCTION sync_touch_updated_at()', t);
    END LOOP;
END $$;

-- 2. Keyset indexes -------------------------------------------------------------
CREATE INDEX IF NOT EXISTS idx_sync_categories ON categories (company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_sync_products ON products (company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_sync_variants ON product_variants (company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_sync_customers ON customers (company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_sync_locations ON inventory_locations (company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_sync_registers ON cash_registers (company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_sync_stock ON stock_balances (company_id, updated_at, variant_id, location_id);

-- 3. Operation journal ----------------------------------------------------------
CREATE TABLE IF NOT EXISTS sync_operations (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE RESTRICT,
    client_op_id UUID NOT NULL,
    device_id VARCHAR(100),
    user_id UUID NOT NULL,
    op_type VARCHAR(40) NOT NULL,
    payload JSONB NOT NULL,
    client_created_at TIMESTAMPTZ,
    status VARCHAR(20) NOT NULL DEFAULT 'processing',
    result JSONB,
    error_code VARCHAR(80),
    error_message TEXT,
    attempts INTEGER NOT NULL DEFAULT 1,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMPTZ,
    resolution_note TEXT,
    CONSTRAINT uq_sync_operations_client UNIQUE (company_id, client_op_id),
    CONSTRAINT fk_sync_operations_user
        FOREIGN KEY (user_id, company_id) REFERENCES users(id, company_id) ON DELETE RESTRICT,
    CONSTRAINT chk_sync_operations_status
        CHECK (status IN ('processing','applied','rejected','dismissed')),
    CONSTRAINT chk_sync_operations_type
        CHECK (op_type IN ('sale.checkout','expense.create','stock.adjust','customer.payment','cash.movement'))
);
CREATE INDEX IF NOT EXISTS idx_sync_operations_status ON sync_operations (company_id, status, id DESC);
CREATE INDEX IF NOT EXISTS idx_sync_operations_user ON sync_operations (company_id, user_id, id DESC);

-- 4. Permissions ----------------------------------------------------------------
INSERT INTO permissions (permission_key, resource, action, description) VALUES
    ('sync.use', 'sync', 'use', 'Pull data and push offline operations'),
    ('sync.manage', 'sync', 'manage', 'Review, retry and dismiss rejected offline operations')
ON CONFLICT (permission_key) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r CROSS JOIN permissions p
WHERE r.is_system_role = TRUE
  AND p.permission_key IN ('sync.use', 'sync.manage')
  AND r.role_key IN ('owner', 'admin', 'manager')
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r CROSS JOIN permissions p
WHERE r.is_system_role = TRUE AND r.role_key IN ('cashier', 'inventory_clerk', 'accountant')
  AND p.permission_key = 'sync.use'
ON CONFLICT DO NOTHING;
