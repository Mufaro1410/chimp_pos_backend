-- Add migration script here

-- ============================================================
-- CHIMPPOS: PURCHASING AND GOODS RECEIVING
-- ============================================================

-- Company-specific purchase order numbering.
CREATE TABLE purchase_order_sequences (
    company_id UUID PRIMARY KEY,
    next_number BIGINT NOT NULL DEFAULT 1,

    CONSTRAINT chk_purchase_sequence_positive
        CHECK (next_number > 0),

    CONSTRAINT fk_purchase_sequence_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT
);

-- ============================================================
-- PURCHASE ORDERS
-- ============================================================

CREATE TABLE purchase_orders (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    supplier_id BIGINT NOT NULL,

    order_number BIGINT NOT NULL,

    status VARCHAR(30) NOT NULL DEFAULT 'draft',

    order_date DATE NOT NULL DEFAULT CURRENT_DATE,
    expected_delivery_date DATE,

    subtotal NUMERIC(19,4) NOT NULL DEFAULT 0,
    tax_total NUMERIC(19,4) NOT NULL DEFAULT 0,
    discount_total NUMERIC(19,4) NOT NULL DEFAULT 0,
    total_amount NUMERIC(19,4) NOT NULL DEFAULT 0,

    notes TEXT,

    created_by UUID NOT NULL,
    submitted_at TIMESTAMPTZ,
    cancelled_at TIMESTAMPTZ,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INTEGER NOT NULL DEFAULT 1,

    CONSTRAINT uq_purchase_orders_id_company
        UNIQUE (id, company_id),

    CONSTRAINT uq_purchase_orders_number
        UNIQUE (company_id, order_number),

    CONSTRAINT fk_purchase_orders_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_purchase_orders_supplier
        FOREIGN KEY (supplier_id, company_id)
        REFERENCES suppliers(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_purchase_orders_created_by
        FOREIGN KEY (created_by, company_id)
        REFERENCES users(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_purchase_order_status
        CHECK (
            status IN (
                'draft',
                'ordered',
                'partially_received',
                'received',
                'cancelled'
            )
        ),

    CONSTRAINT chk_purchase_order_amounts
        CHECK (
            subtotal >= 0
            AND tax_total >= 0
            AND discount_total >= 0
            AND total_amount >= 0
        ),

    CONSTRAINT chk_purchase_order_total
        CHECK (
            total_amount = subtotal + tax_total - discount_total
        ),

    CONSTRAINT chk_purchase_order_discount
        CHECK (discount_total <= subtotal + tax_total),

    CONSTRAINT chk_purchase_order_dates
        CHECK (
            expected_delivery_date IS NULL
            OR expected_delivery_date >= order_date
        ),

    CONSTRAINT chk_purchase_order_version
        CHECK (version > 0)
);

CREATE INDEX idx_purchase_orders_company_date
    ON purchase_orders(company_id, order_date DESC);

CREATE INDEX idx_purchase_orders_supplier
    ON purchase_orders(company_id, supplier_id);

CREATE INDEX idx_purchase_orders_status
    ON purchase_orders(company_id, status);

-- ============================================================
-- PURCHASE ORDER ITEMS
-- ============================================================

CREATE TABLE purchase_order_items (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    purchase_order_id BIGINT NOT NULL,
    variant_id BIGINT NOT NULL,

    -- Historical snapshots: do not rely on live product names.
    product_name VARCHAR(150) NOT NULL,
    variant_name VARCHAR(150) NOT NULL,
    sku VARCHAR(80),
    unit_code VARCHAR(30) NOT NULL,
    unit_name VARCHAR(80) NOT NULL,

    ordered_quantity NUMERIC(19,4) NOT NULL,
    received_quantity NUMERIC(19,4) NOT NULL DEFAULT 0,

    unit_cost NUMERIC(19,4) NOT NULL,
    line_total NUMERIC(19,4) NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INTEGER NOT NULL DEFAULT 1,

    CONSTRAINT uq_purchase_order_items_id_company
        UNIQUE (id, company_id),

    CONSTRAINT fk_purchase_items_order
        FOREIGN KEY (purchase_order_id, company_id)
        REFERENCES purchase_orders(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_purchase_items_variant
        FOREIGN KEY (variant_id, company_id)
        REFERENCES product_variants(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_purchase_item_quantity
        CHECK (
            ordered_quantity > 0
            AND received_quantity >= 0
            AND received_quantity <= ordered_quantity
        ),

    CONSTRAINT chk_purchase_item_cost
        CHECK (unit_cost >= 0 AND line_total >= 0),

    CONSTRAINT chk_purchase_item_line_total
        CHECK (
            line_total = ROUND(ordered_quantity * unit_cost, 4)
        ),

    CONSTRAINT chk_purchase_item_version
        CHECK (version > 0),

    CONSTRAINT chk_purchase_item_names
        CHECK (
            LENGTH(TRIM(product_name)) > 0
            AND LENGTH(TRIM(variant_name)) > 0
        )
);

CREATE INDEX idx_purchase_items_order
    ON purchase_order_items(company_id, purchase_order_id);

CREATE INDEX idx_purchase_items_variant
    ON purchase_order_items(company_id, variant_id);

-- ============================================================
-- GOODS RECEIPTS
-- A receipt represents one actual delivery against an order.
-- ============================================================

CREATE TABLE goods_receipts (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    purchase_order_id BIGINT NOT NULL,
    location_id BIGINT NOT NULL,

    receipt_number BIGINT NOT NULL,
    idempotency_key UUID NOT NULL,

    notes TEXT,

    received_by UUID NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT uq_goods_receipts_id_company
        UNIQUE (id, company_id),

    CONSTRAINT uq_goods_receipts_number
        UNIQUE (company_id, receipt_number),

    CONSTRAINT uq_goods_receipts_idempotency
        UNIQUE (company_id, idempotency_key),

    CONSTRAINT fk_goods_receipts_order
        FOREIGN KEY (purchase_order_id, company_id)
        REFERENCES purchase_orders(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_goods_receipts_location
        FOREIGN KEY (location_id, company_id)
        REFERENCES inventory_locations(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_goods_receipts_received_by
        FOREIGN KEY (received_by, company_id)
        REFERENCES users(id, company_id)
        ON DELETE RESTRICT
);

CREATE INDEX idx_goods_receipts_order
    ON goods_receipts(company_id, purchase_order_id, received_at DESC);

CREATE INDEX idx_goods_receipts_location
    ON goods_receipts(company_id, location_id, received_at DESC);

-- ============================================================
-- GOODS RECEIPT ITEMS
-- ============================================================

CREATE TABLE goods_receipt_items (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    goods_receipt_id BIGINT NOT NULL,
    purchase_order_item_id BIGINT NOT NULL,

    quantity_received NUMERIC(19,4) NOT NULL,
    unit_cost NUMERIC(19,4) NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT fk_receipt_items_receipt
        FOREIGN KEY (goods_receipt_id, company_id)
        REFERENCES goods_receipts(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_receipt_items_order_item
        FOREIGN KEY (purchase_order_item_id, company_id)
        REFERENCES purchase_order_items(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_receipt_item_quantity
        CHECK (quantity_received > 0),

    CONSTRAINT chk_receipt_item_cost
        CHECK (unit_cost >= 0),

    CONSTRAINT uq_receipt_order_item
        UNIQUE (goods_receipt_id, purchase_order_item_id)
);

CREATE INDEX idx_receipt_items_order_item
    ON goods_receipt_items(company_id, purchase_order_item_id);
