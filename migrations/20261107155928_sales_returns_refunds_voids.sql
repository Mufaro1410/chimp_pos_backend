-- Add migration script here

-- Returns, refunds and voids.
-- Original sales and sale_payments are never deleted or rewritten.

CREATE TABLE sale_returns (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    sale_id BIGINT NOT NULL,
    location_id BIGINT NOT NULL,
    reason TEXT NOT NULL,
    notes TEXT,
    idempotency_key UUID NOT NULL,
    request_fingerprint TEXT NOT NULL,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT fk_sale_returns_sale
        FOREIGN KEY (sale_id, company_id)
        REFERENCES sales(id, company_id),

    CONSTRAINT fk_sale_returns_location
        FOREIGN KEY (location_id, company_id)
        REFERENCES inventory_locations(id, company_id),

    CONSTRAINT fk_sale_returns_user
        FOREIGN KEY (created_by, company_id)
        REFERENCES users(id, company_id),

    CONSTRAINT uq_sale_returns_idempotency
        UNIQUE (company_id, idempotency_key),

    CONSTRAINT uq_sale_returns_id_company
        UNIQUE (id, company_id),

    CONSTRAINT chk_sale_returns_reason
        CHECK (length(trim(reason)) > 0)
);

CREATE INDEX idx_sale_returns_sale
    ON sale_returns(company_id, sale_id, created_at DESC);


CREATE TABLE sale_return_items (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    return_id BIGINT NOT NULL,
    sale_item_id BIGINT NOT NULL,
    variant_id BIGINT NOT NULL,

    product_name TEXT NOT NULL,
    variant_name TEXT NOT NULL,
    sku TEXT,
    quantity NUMERIC(19,4) NOT NULL,
    item_condition VARCHAR(20) NOT NULL,
    refundable_amount NUMERIC(19,4) NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT fk_return_items_return
        FOREIGN KEY (return_id, company_id)
        REFERENCES sale_returns(id, company_id),

    CONSTRAINT fk_return_items_variant
        FOREIGN KEY (variant_id, company_id)
        REFERENCES product_variants(id, company_id),

    CONSTRAINT chk_return_item_quantity
        CHECK (quantity > 0),

    CONSTRAINT chk_return_item_condition
        CHECK (item_condition IN ('resellable', 'damaged', 'unsellable')),

    CONSTRAINT chk_return_item_refundable
        CHECK (refundable_amount >= 0)
);

CREATE INDEX idx_sale_return_items_return
    ON sale_return_items(company_id, return_id);

CREATE INDEX idx_sale_return_items_sale_item
    ON sale_return_items(company_id, sale_item_id);


CREATE TABLE sale_voids (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    sale_id BIGINT NOT NULL,
    reason TEXT NOT NULL,
    notes TEXT,
    idempotency_key UUID NOT NULL,
    request_fingerprint TEXT NOT NULL,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    

    CONSTRAINT fk_sale_voids_sale
        FOREIGN KEY (sale_id, company_id)
        REFERENCES sales(id, company_id),

    CONSTRAINT fk_sale_voids_user
        FOREIGN KEY (created_by, company_id)
        REFERENCES users(id, company_id),
    
    CONSTRAINT uq_sale_voids_id_company
        UNIQUE (id, company_id),

    CONSTRAINT uq_sale_voids_sale
        UNIQUE (company_id, sale_id),

    CONSTRAINT uq_sale_voids_idempotency
        UNIQUE (company_id, idempotency_key),

    CONSTRAINT chk_sale_voids_reason
        CHECK (length(trim(reason)) > 0)

);


CREATE TABLE sale_refunds (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    sale_id BIGINT NOT NULL,
    return_id BIGINT,
    void_id BIGINT,

    payment_method VARCHAR(30) NOT NULL,
    amount NUMERIC(19,4) NOT NULL,
    reference TEXT,
    notes TEXT,

    idempotency_key UUID NOT NULL,
    request_fingerprint TEXT NOT NULL,
    refunded_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT fk_sale_refunds_sale
        FOREIGN KEY (sale_id, company_id)
        REFERENCES sales(id, company_id),

    CONSTRAINT fk_sale_refunds_return
        FOREIGN KEY (return_id, company_id)
        REFERENCES sale_returns(id, company_id),

    CONSTRAINT fk_sale_refunds_void
        FOREIGN KEY (void_id, company_id)
        REFERENCES sale_voids(id, company_id),

    CONSTRAINT fk_sale_refunds_user
        FOREIGN KEY (refunded_by, company_id)
        REFERENCES users(id, company_id),

    CONSTRAINT uq_sale_refunds_idempotency
        UNIQUE (company_id, idempotency_key),

    CONSTRAINT chk_sale_refunds_amount
        CHECK (amount > 0),

    CONSTRAINT chk_sale_refunds_method
        CHECK (payment_method IN ('cash', 'card', 'mobile_money', 'other')),

    CONSTRAINT chk_sale_refunds_parent
        CHECK (
            (return_id IS NOT NULL AND void_id IS NULL)
            OR
            (return_id IS NULL AND void_id IS NOT NULL)
        )
);

CREATE INDEX idx_sale_refunds_sale
    ON sale_refunds(company_id, sale_id, created_at DESC);

CREATE INDEX idx_sale_refunds_return
    ON sale_refunds(company_id, return_id)
    WHERE return_id IS NOT NULL;



-- Add the sale return permission.
INSERT INTO permissions (
    permission_key,
    resource,
    action,
    description
)
VALUES (
    'sales.return',
    'sales',
    'return',
    'Process sale returns'
)
ON CONFLICT (permission_key) DO NOTHING;


-- Grant these permissions to Owner, Admin and Manager roles.
-- This assumes your role names are stored in roles.name.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE lower(r.name) IN ('owner', 'admin', 'manager')
  AND p.permission_key IN ('sales.return', 'sales.refund', 'sales.void')
ON CONFLICT DO NOTHING;
