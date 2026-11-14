-- Add migration script here

-- ============================================================
-- CHIMPPOS: SALES AND CHECKOUT
-- ============================================================

-- ------------------------------------------------------------
-- 1. COMPANY-SPECIFIC SALE NUMBER SEQUENCES
-- ------------------------------------------------------------

CREATE TABLE sale_sequences (
    company_id UUID PRIMARY KEY,
    next_number BIGINT NOT NULL DEFAULT 1,

    CONSTRAINT fk_sale_sequences_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_sale_sequence_positive
        CHECK (next_number > 0)
);

-- ------------------------------------------------------------
-- 2. SALES
-- ------------------------------------------------------------

CREATE TABLE sales (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    location_id BIGINT NOT NULL,

    sale_number BIGINT NOT NULL,

    status VARCHAR(20) NOT NULL DEFAULT 'draft',

    subtotal NUMERIC(19,4) NOT NULL DEFAULT 0,
    discount_total NUMERIC(19,4) NOT NULL DEFAULT 0,
    tax_total NUMERIC(19,4) NOT NULL DEFAULT 0,
    total_amount NUMERIC(19,4) NOT NULL DEFAULT 0,

    notes TEXT,

    created_by UUID NOT NULL,
    completed_by UUID,
    completed_at TIMESTAMPTZ,

    cancelled_by UUID,
    cancelled_at TIMESTAMPTZ,

    checkout_idempotency_key UUID,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    version INTEGER NOT NULL DEFAULT 1,

    CONSTRAINT uq_sales_id_company
        UNIQUE (id, company_id),

    CONSTRAINT uq_sales_number
        UNIQUE (company_id, sale_number),

    CONSTRAINT fk_sales_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_sales_location
        FOREIGN KEY (location_id, company_id)
        REFERENCES inventory_locations(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_sales_created_by
        FOREIGN KEY (created_by, company_id)
        REFERENCES users(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_sales_completed_by
        FOREIGN KEY (completed_by, company_id)
        REFERENCES users(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_sales_cancelled_by
        FOREIGN KEY (cancelled_by, company_id)
        REFERENCES users(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_sales_status
        CHECK (
            status IN ('draft', 'completed', 'cancelled')
        ),

    CONSTRAINT chk_sales_amounts
        CHECK (
            subtotal >= 0
            AND discount_total >= 0
            AND tax_total >= 0
            AND total_amount >= 0
        ),

    CONSTRAINT chk_sales_total
        CHECK (
            total_amount = subtotal - discount_total + tax_total
        ),

    CONSTRAINT chk_sales_discount
        CHECK (discount_total <= subtotal),

    CONSTRAINT chk_sales_version
        CHECK (version > 0),

    CONSTRAINT chk_sales_completion
        CHECK (
            (status = 'completed'
                AND completed_by IS NOT NULL
                AND completed_at IS NOT NULL)
            OR
            (status <> 'completed'
                AND completed_by IS NULL
                AND completed_at IS NULL)
        ),

    CONSTRAINT chk_sales_cancellation
        CHECK (
            (status = 'cancelled'
                AND cancelled_by IS NOT NULL
                AND cancelled_at IS NOT NULL)
            OR
            (status <> 'cancelled'
                AND cancelled_by IS NULL
                AND cancelled_at IS NULL)
        )
);

-- A checkout key may only complete one sale per company.
CREATE UNIQUE INDEX uq_sales_checkout_idempotency
ON sales(company_id, checkout_idempotency_key)
WHERE checkout_idempotency_key IS NOT NULL;

CREATE INDEX idx_sales_company_date
ON sales(company_id, created_at DESC);

CREATE INDEX idx_sales_company_status
ON sales(company_id, status);

CREATE INDEX idx_sales_location_date
ON sales(company_id, location_id, created_at DESC);

CREATE INDEX idx_sales_cashier_date
ON sales(company_id, created_by, created_at DESC);

-- ------------------------------------------------------------
-- 3. SALE ITEMS
-- Historical snapshots are stored independently of the
-- current product catalogue.
-- ------------------------------------------------------------

CREATE TABLE sale_items (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

    company_id UUID NOT NULL,
    sale_id BIGINT NOT NULL,
    variant_id BIGINT NOT NULL,

    product_name VARCHAR(150) NOT NULL,
    variant_name VARCHAR(100) NOT NULL,

    sku VARCHAR(80),
    barcode VARCHAR(100),

    unit_code VARCHAR(30) NOT NULL,
    unit_name VARCHAR(80) NOT NULL,

    quantity NUMERIC(19,4) NOT NULL,

    unit_price NUMERIC(19,4) NOT NULL,

    discount_amount NUMERIC(19,4) NOT NULL DEFAULT 0,

    tax_rate NUMERIC(9,4) NOT NULL DEFAULT 0,
    tax_amount NUMERIC(19,4) NOT NULL DEFAULT 0,

    line_subtotal NUMERIC(19,4) NOT NULL,
    line_total NUMERIC(19,4) NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT uq_sale_items_id_company
        UNIQUE (id, company_id),

    CONSTRAINT fk_sale_items_sale
        FOREIGN KEY (sale_id, company_id)
        REFERENCES sales(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_sale_items_variant
        FOREIGN KEY (variant_id, company_id)
        REFERENCES product_variants(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_sale_item_quantity
        CHECK (quantity > 0),

    CONSTRAINT chk_sale_item_price
        CHECK (unit_price >= 0),

    CONSTRAINT chk_sale_item_discount
        CHECK (
            discount_amount >= 0
            AND discount_amount <= line_subtotal
        ),

    CONSTRAINT chk_sale_item_tax_rate
        CHECK (tax_rate >= 0 AND tax_rate <= 100),

    CONSTRAINT chk_sale_item_tax
        CHECK (tax_amount >= 0),

    CONSTRAINT chk_sale_item_subtotal
        CHECK (
            line_subtotal = ROUND(quantity * unit_price, 4)
        ),

    CONSTRAINT chk_sale_item_total
        CHECK (
            line_total = line_subtotal - discount_amount + tax_amount
        ),

    CONSTRAINT chk_sale_item_names
        CHECK (
            LENGTH(TRIM(product_name)) > 0
            AND LENGTH(TRIM(variant_name)) > 0
            AND LENGTH(TRIM(unit_code)) > 0
            AND LENGTH(TRIM(unit_name)) > 0
        )
);

-- A variant appears once per sale. Multiple quantities
-- are represented by the quantity column.
CREATE UNIQUE INDEX uq_sale_items_variant
ON sale_items(sale_id, variant_id);

CREATE INDEX idx_sale_items_sale
ON sale_items(company_id, sale_id);

CREATE INDEX idx_sale_items_variant
ON sale_items(company_id, variant_id);

-- ------------------------------------------------------------
-- 4. SALE PAYMENTS
-- Amount is the amount applied to the sale.
-- Tendered amount records what the customer handed over.
-- Change is only applicable to cash.
-- ------------------------------------------------------------

CREATE TABLE sale_payments (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

    company_id UUID NOT NULL,
    sale_id BIGINT NOT NULL,

    payment_method VARCHAR(30) NOT NULL,

    amount NUMERIC(19,4) NOT NULL,
    tendered_amount NUMERIC(19,4) NOT NULL,
    change_given NUMERIC(19,4) NOT NULL DEFAULT 0,

    reference VARCHAR(150),
    notes TEXT,

    received_by UUID NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT fk_sale_payments_sale
        FOREIGN KEY (sale_id, company_id)
        REFERENCES sales(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_sale_payments_user
        FOREIGN KEY (received_by, company_id)
        REFERENCES users(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_sale_payment_method
        CHECK (
            payment_method IN (
                'cash',
                'card',
                'mobile_money',
                'other'
            )
        ),

    CONSTRAINT chk_sale_payment_amount
        CHECK (
            amount > 0
            AND tendered_amount >= amount
            AND change_given >= 0
        ),

    CONSTRAINT chk_sale_payment_change
        CHECK (
            tendered_amount = amount + change_given
        ),

    CONSTRAINT chk_sale_payment_non_cash
        CHECK (
            payment_method = 'cash'
            OR (
                tendered_amount = amount
                AND change_given = 0
            )
        )
);

CREATE INDEX idx_sale_payments_sale
ON sale_payments(company_id, sale_id);

CREATE INDEX idx_sale_payments_date
ON sale_payments(company_id, created_at DESC);

-- ------------------------------------------------------------
-- 5. SALES PERMISSIONS
-- ------------------------------------------------------------

INSERT INTO permissions (
    permission_key,
    resource,
    action,
    description
)
VALUES
    (
        'sales.view',
        'sales',
        'view',
        'View sales and receipts'
    ),
    (
        'sales.create',
        'sales',
        'create',
        'Create draft sales'
    ),
    (
        'sales.update',
        'sales',
        'update',
        'Update draft sales'
    ),
    (
        'sales.checkout',
        'sales',
        'checkout',
        'Complete sales and receive payments'
    ),
    (
        'sales.cancel',
        'sales',
        'cancel',
        'Cancel draft sales'
    )
ON CONFLICT (permission_key) DO NOTHING;

-- Owner and Admin: all sales permissions.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.is_system_role = TRUE
  AND r.role_key IN ('owner', 'admin')
  AND p.permission_key IN (
      'sales.view',
      'sales.create',
      'sales.update',
      'sales.checkout',
      'sales.cancel'
  )
ON CONFLICT (role_id, permission_id) DO NOTHING;

-- Manager: all sales permissions except cancellation.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.is_system_role = TRUE
  AND r.role_key = 'manager'
  AND p.permission_key IN (
      'sales.view',
      'sales.create',
      'sales.update',
      'sales.checkout'
  )
ON CONFLICT (role_id, permission_id) DO NOTHING;

-- Cashier: create, view, update and checkout.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.is_system_role = TRUE
  AND r.role_key = 'cashier'
  AND p.permission_key IN (
      'sales.view',
      'sales.create',
      'sales.update',
      'sales.checkout'
  )
ON CONFLICT (role_id, permission_id) DO NOTHING;
