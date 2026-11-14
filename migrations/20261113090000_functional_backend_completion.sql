-- ============================================================
-- Functional backend completion (additive; does not alter applied history)
--
--  1. Backfill a default stock location for companies that lack one.
--  2. Backfill stock balances for variants missing a default-location row.
--  3. Reconcile default system-role grants (additive only).
--  4. Expenses (with optional cash-session payout).
--  5. Immutable audit log.
--  6. Cost snapshot on sale items (gross profit reporting).
-- ============================================================

-- ------------------------------------------------------------
-- 1. Default location
-- ------------------------------------------------------------
INSERT INTO inventory_locations (company_id, name, code, location_type, is_default)
SELECT c.id, 'Main Store', 'MAIN', 'store', TRUE
FROM companies c
WHERE NOT EXISTS (
    SELECT 1 FROM inventory_locations l
    WHERE l.company_id = c.id AND l.is_default = TRUE
)
ON CONFLICT DO NOTHING;

-- ------------------------------------------------------------
-- 2. Stock balances for the default location
-- ------------------------------------------------------------
INSERT INTO stock_balances (company_id, location_id, variant_id)
SELECT v.company_id, l.id, v.id
FROM product_variants v
JOIN inventory_locations l
  ON l.company_id = v.company_id AND l.is_default = TRUE
ON CONFLICT DO NOTHING;

-- ------------------------------------------------------------
-- 3. Reconcile system role grants (never revokes anything)
-- ------------------------------------------------------------
-- Owner: every permission.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r CROSS JOIN permissions p
WHERE r.is_system_role = TRUE AND r.role_key = 'owner'
ON CONFLICT DO NOTHING;

-- Admin: everything except role / settings management.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r CROSS JOIN permissions p
WHERE r.is_system_role = TRUE AND r.role_key = 'admin'
  AND p.permission_key NOT IN ('roles.manage', 'settings.manage')
ON CONFLICT DO NOTHING;

-- Manager
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.permission_key = ANY (ARRAY[
    'dashboard.view','categories.view','categories.manage',
    'customers.view','customers.manage','customers.profile.manage','customers.payment',
    'products.view','products.create','products.update',
    'inventory.view','inventory.adjust','inventory.receive','inventory.transfer',
    'sales.view','sales.create','sales.update','sales.cancel','sales.checkout',
    'sales.discount','sales.return','sales.refund','sales.void',
    'cash_register.view','cash_register.open','cash_register.close','cash_register.movement',
    'suppliers.view','suppliers.manage','suppliers.invoice','suppliers.payment',
    'purchases.view','purchases.create','purchases.update','purchases.cancel','purchases.receive',
    'expenses.view','expenses.manage','reports.view','reports.export','users.view'
])
WHERE r.is_system_role = TRUE AND r.role_key = 'manager'
ON CONFLICT DO NOTHING;

-- Cashier
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.permission_key = ANY (ARRAY[
    'dashboard.view','products.view','inventory.view',
    'customers.view','customers.manage','customers.profile.manage','customers.payment',
    'sales.view','sales.create','sales.update','sales.cancel','sales.checkout',
    'cash_register.view','cash_register.open','cash_register.close','cash_register.movement'
])
WHERE r.is_system_role = TRUE AND r.role_key = 'cashier'
ON CONFLICT DO NOTHING;

-- ------------------------------------------------------------
-- 4. Expenses
-- ------------------------------------------------------------
CREATE TABLE expense_sequences (
    company_id UUID PRIMARY KEY REFERENCES companies(id) ON DELETE RESTRICT,
    last_number BIGINT NOT NULL DEFAULT 0 CHECK (last_number >= 0)
);

CREATE TABLE expenses (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    expense_number BIGINT NOT NULL,

    category VARCHAR(100) NOT NULL,
    description TEXT NOT NULL,
    amount NUMERIC(19,4) NOT NULL,
    currency_code CHAR(3) NOT NULL,
    expense_date DATE NOT NULL DEFAULT CURRENT_DATE,

    payment_method VARCHAR(30) NOT NULL,
    reference VARCHAR(150),
    notes TEXT,

    supplier_id BIGINT,
    cash_session_id BIGINT,
    cash_movement_id BIGINT,

    status VARCHAR(20) NOT NULL DEFAULT 'posted',
    voided_by UUID,
    voided_at TIMESTAMPTZ,
    void_reason TEXT,

    idempotency_key UUID NOT NULL,
    request_fingerprint TEXT NOT NULL,

    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INTEGER NOT NULL DEFAULT 1,

    CONSTRAINT uq_expenses_id_company UNIQUE (id, company_id),
    CONSTRAINT uq_expenses_number UNIQUE (company_id, expense_number),
    CONSTRAINT uq_expenses_idempotency UNIQUE (company_id, idempotency_key),

    CONSTRAINT fk_expenses_company_currency
        FOREIGN KEY (company_id, currency_code)
        REFERENCES companies(id, default_currency)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT fk_expenses_supplier
        FOREIGN KEY (supplier_id, company_id)
        REFERENCES suppliers(id, company_id) ON DELETE RESTRICT,
    CONSTRAINT fk_expenses_session
        FOREIGN KEY (cash_session_id, company_id)
        REFERENCES cash_register_sessions(id, company_id) ON DELETE RESTRICT,
    CONSTRAINT fk_expenses_created_by
        FOREIGN KEY (created_by, company_id)
        REFERENCES users(id, company_id) ON DELETE RESTRICT,

    CONSTRAINT chk_expenses_amount CHECK (amount > 0),
    CONSTRAINT chk_expenses_category CHECK (length(btrim(category)) > 0),
    CONSTRAINT chk_expenses_description CHECK (length(btrim(description)) > 0),
    CONSTRAINT chk_expenses_method
        CHECK (payment_method IN ('cash','card','mobile_money','bank_transfer','other')),
    CONSTRAINT chk_expenses_cash_session
        CHECK ((payment_method = 'cash') = (cash_session_id IS NOT NULL)),
    CONSTRAINT chk_expenses_status CHECK (status IN ('posted','voided')),
    CONSTRAINT chk_expenses_void
        CHECK ((status = 'voided') = (voided_at IS NOT NULL)),
    CONSTRAINT chk_expenses_version CHECK (version > 0)
);

CREATE INDEX idx_expenses_company_date ON expenses (company_id, expense_date DESC, id DESC);
CREATE INDEX idx_expenses_company_category ON expenses (company_id, category);

-- ------------------------------------------------------------
-- 5. Audit log (append-only)
-- ------------------------------------------------------------
CREATE TABLE audit_logs (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE RESTRICT,
    user_id UUID,
    method VARCHAR(10) NOT NULL,
    path TEXT NOT NULL,
    status_code INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_audit_logs_company_time ON audit_logs (company_id, created_at DESC, id DESC);
CREATE INDEX idx_audit_logs_company_user ON audit_logs (company_id, user_id);

CREATE FUNCTION prevent_audit_log_mutation()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'audit_logs is append-only' USING ERRCODE = '23514';
END;
$$;

CREATE TRIGGER trg_audit_logs_immutable
    BEFORE UPDATE OR DELETE ON audit_logs
    FOR EACH ROW EXECUTE FUNCTION prevent_audit_log_mutation();

-- ------------------------------------------------------------
-- 6. Cost snapshot on sale items
-- ------------------------------------------------------------
ALTER TABLE sale_items ADD COLUMN unit_cost NUMERIC(19,4);

CREATE FUNCTION snapshot_sale_item_cost()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.unit_cost IS NULL THEN
        SELECT v.cost_price INTO NEW.unit_cost
        FROM product_variants v
        WHERE v.id = NEW.variant_id AND v.company_id = NEW.company_id;
    END IF;
    RETURN NEW;
END;
$$;

-- Named so it fires before the immutability trigger (alphabetical order).
CREATE TRIGGER trg_a_sale_item_cost_snapshot
    BEFORE INSERT ON sale_items
    FOR EACH ROW EXECUTE FUNCTION snapshot_sale_item_cost();
