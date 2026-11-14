-- Add migration script here

-- Stage 12: Cash registers and register sessions

-- ------------------------------------------------------------
-- 1. CASH REGISTERS
-- A company can have multiple registers at a location.
-- ------------------------------------------------------------

CREATE TABLE cash_registers (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    location_id BIGINT NOT NULL,

    code VARCHAR(30) NOT NULL,
    name VARCHAR(100) NOT NULL,

    is_active BOOLEAN NOT NULL DEFAULT TRUE,

    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT fk_cash_registers_location
        FOREIGN KEY (location_id, company_id)
        REFERENCES inventory_locations(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_cash_registers_created_by
        FOREIGN KEY (created_by, company_id)
        REFERENCES users(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT uq_cash_registers_id_company
        UNIQUE (id, company_id),

    CONSTRAINT uq_cash_registers_id_company_location
        UNIQUE (id, company_id, location_id),

    CONSTRAINT uq_cash_registers_location_code
        UNIQUE (company_id, location_id, code),

    CONSTRAINT chk_cash_registers_code
        CHECK (length(trim(code)) > 0),

    CONSTRAINT chk_cash_registers_name
        CHECK (length(trim(name)) > 0)
);

CREATE INDEX idx_cash_registers_location
    ON cash_registers(company_id, location_id, is_active);


-- ------------------------------------------------------------
-- 2. CASH REGISTER SESSIONS
-- One active session per register.
-- ------------------------------------------------------------

CREATE TABLE cash_register_sessions (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

    company_id UUID NOT NULL,
    register_id BIGINT NOT NULL,
    location_id BIGINT NOT NULL,

    status VARCHAR(20) NOT NULL DEFAULT 'open',

    opening_float NUMERIC(19,4) NOT NULL DEFAULT 0,
    opening_notes TEXT,

    opened_by UUID NOT NULL,
    opened_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    closed_by UUID,
    closed_at TIMESTAMPTZ,

    expected_cash NUMERIC(19,4),
    closing_cash_count NUMERIC(19,4),
    cash_variance NUMERIC(19,4),

    closing_notes TEXT,

    version INTEGER NOT NULL DEFAULT 1,

    CONSTRAINT fk_cash_sessions_register
        FOREIGN KEY (register_id, company_id, location_id)
        REFERENCES cash_registers(id, company_id, location_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_cash_sessions_opened_by
        FOREIGN KEY (opened_by, company_id)
        REFERENCES users(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_cash_sessions_closed_by
        FOREIGN KEY (closed_by, company_id)
        REFERENCES users(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT uq_cash_sessions_id_company
        UNIQUE (id, company_id),

    CONSTRAINT chk_cash_sessions_status
        CHECK (status IN ('open', 'closed')),

    CONSTRAINT chk_cash_sessions_opening_float
        CHECK (opening_float >= 0),

    CONSTRAINT chk_cash_sessions_version
        CHECK (version > 0),

    CONSTRAINT chk_cash_sessions_closing_values
        CHECK (
            (
                status = 'open'
                AND closed_by IS NULL
                AND closed_at IS NULL
                AND expected_cash IS NULL
                AND closing_cash_count IS NULL
                AND cash_variance IS NULL
            )
            OR
            (
                status = 'closed'
                AND closed_by IS NOT NULL
                AND closed_at IS NOT NULL
                AND expected_cash IS NOT NULL
                AND closing_cash_count IS NOT NULL
                AND cash_variance IS NOT NULL
            )
        ),

    CONSTRAINT chk_cash_sessions_closing_amounts
        CHECK (
            (expected_cash IS NULL OR expected_cash >= 0)
            AND
            (closing_cash_count IS NULL OR closing_cash_count >= 0)
        )
);

-- A register cannot have two open sessions simultaneously.
CREATE UNIQUE INDEX uq_cash_sessions_one_open_per_register
    ON cash_register_sessions(company_id, register_id)
    WHERE status = 'open';

CREATE INDEX idx_cash_sessions_location
    ON cash_register_sessions(company_id, location_id, opened_at DESC);

CREATE INDEX idx_cash_sessions_register_history
    ON cash_register_sessions(company_id, register_id, opened_at DESC);

CREATE INDEX idx_cash_sessions_open
    ON cash_register_sessions(company_id, status)
    WHERE status = 'open';


-- ------------------------------------------------------------
-- 3. MANUAL CASH MOVEMENTS
-- Cash-in, cash-out, paid-outs and cash drops.
--
-- Sales and refunds are tracked through their original records.
-- They are NOT duplicated in this table.
-- ------------------------------------------------------------

CREATE TABLE cash_movements (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

    company_id UUID NOT NULL,
    register_session_id BIGINT NOT NULL,

    movement_type VARCHAR(30) NOT NULL,
    amount NUMERIC(19,4) NOT NULL,

    reference VARCHAR(150),
    notes TEXT,

    idempotency_key UUID NOT NULL,
    request_fingerprint TEXT NOT NULL,

    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT fk_cash_movements_session
        FOREIGN KEY (register_session_id, company_id)
        REFERENCES cash_register_sessions(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_cash_movements_created_by
        FOREIGN KEY (created_by, company_id)
        REFERENCES users(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT uq_cash_movements_idempotency
        UNIQUE (company_id, idempotency_key),

    CONSTRAINT chk_cash_movements_type
        CHECK (
            movement_type IN (
                'cash_in',
                'cash_out',
                'paid_out',
                'cash_drop'
            )
        ),

    CONSTRAINT chk_cash_movements_amount
        CHECK (amount > 0),

    CONSTRAINT chk_cash_movements_fingerprint
        CHECK (length(trim(request_fingerprint)) > 0)
);

CREATE INDEX idx_cash_movements_session
    ON cash_movements(company_id, register_session_id, created_at);


-- ------------------------------------------------------------
-- 4. LINK CASH SALES TO REGISTER SESSIONS
-- Non-cash payments must not be assigned to a cash session.
-- ------------------------------------------------------------

ALTER TABLE sale_payments
    ADD COLUMN cash_session_id BIGINT;

ALTER TABLE sale_payments
    ADD CONSTRAINT fk_sale_payments_cash_session
    FOREIGN KEY (cash_session_id, company_id)
    REFERENCES cash_register_sessions(id, company_id)
    ON DELETE RESTRICT;

ALTER TABLE sale_payments
    ADD CONSTRAINT chk_sale_payments_cash_session
    CHECK (
        payment_method = 'cash'
        OR cash_session_id IS NULL
    );

CREATE INDEX idx_sale_payments_cash_session
    ON sale_payments(company_id, cash_session_id)
    WHERE cash_session_id IS NOT NULL;


-- ------------------------------------------------------------
-- 5. LINK CASH REFUNDS TO REGISTER SESSIONS
-- ------------------------------------------------------------

ALTER TABLE sale_refunds
    ADD COLUMN cash_session_id BIGINT;

ALTER TABLE sale_refunds
    ADD CONSTRAINT fk_sale_refunds_cash_session
    FOREIGN KEY (cash_session_id, company_id)
    REFERENCES cash_register_sessions(id, company_id)
    ON DELETE RESTRICT;

ALTER TABLE sale_refunds
    ADD CONSTRAINT chk_sale_refunds_cash_session
    CHECK (
        payment_method = 'cash'
        OR cash_session_id IS NULL
    );

CREATE INDEX idx_sale_refunds_cash_session
    ON sale_refunds(company_id, cash_session_id)
    WHERE cash_session_id IS NOT NULL;


-- ------------------------------------------------------------
-- 6. CASH REGISTER PERMISSIONS
-- ------------------------------------------------------------

INSERT INTO permissions (
    permission_key,
    resource,
    action,
    description
)
VALUES
    (
        'cash_register.view',
        'cash_register',
        'view',
        'View cash registers and sessions'
    ),
    (
        'cash_register.manage',
        'cash_register',
        'manage',
        'Create and manage cash registers'
    ),
    (
        'cash_register.open',
        'cash_register',
        'open',
        'Open cash register sessions'
    ),
    (
        'cash_register.close',
        'cash_register',
        'close',
        'Close and reconcile cash register sessions'
    ),
    (
        'cash_register.movement',
        'cash_register',
        'movement',
        'Record manual cash movements'
    )
ON CONFLICT (permission_key) DO NOTHING;


-- Owner, Admin and Manager receive all cash register permissions.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE LOWER(r.name) IN ('owner', 'admin', 'manager')
  AND p.permission_key IN (
      'cash_register.view',
      'cash_register.manage',
      'cash_register.open',
      'cash_register.close',
      'cash_register.movement'
  )
ON CONFLICT (role_id, permission_id) DO NOTHING;


-- Cashiers can operate registers but cannot manage register setup.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE LOWER(r.name) = 'cashier'
  AND p.permission_key IN (
      'cash_register.view',
      'cash_register.open',
      'cash_register.close',
      'cash_register.movement'
  )
ON CONFLICT (role_id, permission_id) DO NOTHING;
