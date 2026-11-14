-- Add migration script here

-- ============================================================
-- CHIMPPOS: PRODUCT CATALOGUE AND INVENTORY FOUNDATION
-- ============================================================

-- ------------------------------------------------------------
-- 1. UNITS OF MEASURE
-- Global reference catalogue shared by all companies.
-- ------------------------------------------------------------

CREATE TABLE units_of_measure (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    code VARCHAR(20) NOT NULL UNIQUE,
    name VARCHAR(50) NOT NULL,
    symbol VARCHAR(15) NOT NULL,
    allows_decimal BOOLEAN NOT NULL DEFAULT FALSE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE
);

INSERT INTO units_of_measure (code, name, symbol, allows_decimal)
VALUES
    ('each', 'Each', 'ea', FALSE),
    ('kg', 'Kilogram', 'kg', TRUE),
    ('g', 'Gram', 'g', TRUE),
    ('l', 'Litre', 'L', TRUE),
    ('ml', 'Millilitre', 'ml', TRUE),
    ('pack', 'Pack', 'pack', FALSE),
    ('box', 'Box', 'box', FALSE),
    ('dozen', 'Dozen', 'doz', FALSE);

-- ------------------------------------------------------------
-- 2. PRODUCT CATEGORIES
-- Supports nested categories.
-- ------------------------------------------------------------

CREATE TABLE categories (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    parent_id BIGINT,
    name VARCHAR(100) NOT NULL,
    description TEXT,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INTEGER NOT NULL DEFAULT 1,

    CONSTRAINT fk_categories_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT uq_categories_id_company
        UNIQUE (id, company_id),

    CONSTRAINT fk_categories_parent
        FOREIGN KEY (parent_id, company_id)
        REFERENCES categories(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_categories_name
        CHECK (LENGTH(TRIM(name)) > 0),

    CONSTRAINT chk_categories_not_own_parent
        CHECK (parent_id IS NULL OR parent_id <> id),

    CONSTRAINT chk_categories_version
        CHECK (version > 0)
);

CREATE UNIQUE INDEX uq_categories_company_name
ON categories (company_id, LOWER(name));

CREATE INDEX idx_categories_company
ON categories (company_id);

CREATE INDEX idx_categories_parent
ON categories (company_id, parent_id);

-- ------------------------------------------------------------
-- 3. EXTEND EXISTING PRODUCTS
-- Preserve existing product records and API compatibility.
-- ------------------------------------------------------------

ALTER TABLE products
    ADD COLUMN category_id BIGINT,
    ADD COLUMN unit_id BIGINT,
    ADD COLUMN sku VARCHAR(80),
    ADD COLUMN barcode VARCHAR(100),
    ADD COLUMN description TEXT,
    ADD COLUMN track_inventory BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN archived_at TIMESTAMPTZ,
    ADD COLUMN version INTEGER NOT NULL DEFAULT 1;

ALTER TABLE products
    ADD CONSTRAINT uq_products_id_company
        UNIQUE (id, company_id),

    ADD CONSTRAINT fk_products_category
        FOREIGN KEY (category_id, company_id)
        REFERENCES categories(id, company_id)
        ON DELETE RESTRICT,

    ADD CONSTRAINT fk_products_unit
        FOREIGN KEY (unit_id)
        REFERENCES units_of_measure(id)
        ON DELETE RESTRICT,

    ADD CONSTRAINT chk_products_version
        CHECK (version > 0);

CREATE UNIQUE INDEX uq_products_company_sku
ON products (company_id, LOWER(sku))
WHERE sku IS NOT NULL;

CREATE UNIQUE INDEX uq_products_company_barcode
ON products (company_id, barcode)
WHERE barcode IS NOT NULL;

CREATE INDEX idx_products_category
ON products (company_id, category_id);

CREATE INDEX idx_products_active
ON products (company_id, is_active);

-- ------------------------------------------------------------
-- 4. PRODUCT VARIANTS
-- Each variant is independently sellable and stock-tracked.
-- ------------------------------------------------------------

CREATE TABLE product_variants (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    product_id BIGINT NOT NULL,
    name VARCHAR(100) NOT NULL DEFAULT 'Default',
    sku VARCHAR(80),
    barcode VARCHAR(100),
    unit_id BIGINT NOT NULL,
    selling_price NUMERIC(19, 4) NOT NULL,
    cost_price NUMERIC(19, 4),
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INTEGER NOT NULL DEFAULT 1,

    CONSTRAINT fk_variants_product
        FOREIGN KEY (product_id, company_id)
        REFERENCES products(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_variants_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_variants_unit
        FOREIGN KEY (unit_id)
        REFERENCES units_of_measure(id)
        ON DELETE RESTRICT,

    CONSTRAINT uq_variants_id_company
        UNIQUE (id, company_id),

    CONSTRAINT chk_variants_name
        CHECK (LENGTH(TRIM(name)) > 0),

    CONSTRAINT chk_variants_selling_price
        CHECK (selling_price >= 0),

    CONSTRAINT chk_variants_cost_price
        CHECK (cost_price IS NULL OR cost_price >= 0),

    CONSTRAINT chk_variants_version
        CHECK (version > 0)
);

CREATE UNIQUE INDEX uq_variants_company_sku
ON product_variants (company_id, LOWER(sku))
WHERE sku IS NOT NULL;

CREATE UNIQUE INDEX uq_variants_company_barcode
ON product_variants (company_id, barcode)
WHERE barcode IS NOT NULL;

CREATE UNIQUE INDEX uq_variants_default
ON product_variants (product_id)
WHERE is_default = TRUE;

CREATE INDEX idx_variants_product
ON product_variants (company_id, product_id);

-- ------------------------------------------------------------
-- 5. MIGRATE EXISTING PRODUCTS INTO DEFAULT VARIANTS
-- Existing products receive a default unit and variant.
-- ------------------------------------------------------------

UPDATE products
SET unit_id = (
    SELECT id
    FROM units_of_measure
    WHERE code = 'each'
)
WHERE unit_id IS NULL;

INSERT INTO product_variants (
    company_id,
    product_id,
    name,
    sku,
    barcode,
    unit_id,
    selling_price,
    is_default
)
SELECT
    p.company_id,
    p.id,
    'Default',
    p.sku,
    p.barcode,
    p.unit_id,
    p.price,
    TRUE
FROM products p;

-- ------------------------------------------------------------
-- 6. INVENTORY LOCATIONS
-- Every company receives a default Main Store location.
-- ------------------------------------------------------------

CREATE TABLE inventory_locations (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    name VARCHAR(100) NOT NULL,
    code VARCHAR(30) NOT NULL,
    location_type VARCHAR(30) NOT NULL DEFAULT 'store',
    address TEXT,
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INTEGER NOT NULL DEFAULT 1,

    CONSTRAINT fk_locations_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT uq_locations_id_company
        UNIQUE (id, company_id),

    CONSTRAINT uq_locations_company_code
        UNIQUE (company_id, code),

    CONSTRAINT chk_locations_type
        CHECK (location_type IN ('store', 'warehouse')),

    CONSTRAINT chk_locations_name
        CHECK (LENGTH(TRIM(name)) > 0),

    CONSTRAINT chk_locations_version
        CHECK (version > 0)
);

CREATE UNIQUE INDEX uq_locations_default
ON inventory_locations(company_id)
WHERE is_default = TRUE;

CREATE INDEX idx_locations_company_active
ON inventory_locations(company_id, is_active);

INSERT INTO inventory_locations (
    company_id,
    name,
    code,
    location_type,
    is_default
)
SELECT
    id,
    'Main Store',
    'MAIN',
    'store',
    TRUE
FROM companies;

-- ------------------------------------------------------------
-- 7. STOCK BALANCES
-- Current stock for each variant at each location.
-- ------------------------------------------------------------

CREATE TABLE stock_balances (
    company_id UUID NOT NULL,
    location_id BIGINT NOT NULL,
    variant_id BIGINT NOT NULL,
    quantity_on_hand NUMERIC(19, 4) NOT NULL DEFAULT 0,
    reserved_quantity NUMERIC(19, 4) NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INTEGER NOT NULL DEFAULT 1,

    PRIMARY KEY (location_id, variant_id),

    CONSTRAINT fk_balances_location
        FOREIGN KEY (location_id, company_id)
        REFERENCES inventory_locations(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_balances_variant
        FOREIGN KEY (variant_id, company_id)
        REFERENCES product_variants(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_balances_quantity
        CHECK (quantity_on_hand >= 0),

    CONSTRAINT chk_balances_reserved
        CHECK (
            reserved_quantity >= 0
            AND reserved_quantity <= quantity_on_hand
        ),

    CONSTRAINT chk_balances_version
        CHECK (version > 0)
);

CREATE INDEX idx_balances_company
ON stock_balances(company_id);

CREATE INDEX idx_balances_variant
ON stock_balances(company_id, variant_id);

-- Create an initial zero balance for each variant
-- at its company's default location.

INSERT INTO stock_balances (
    company_id,
    location_id,
    variant_id
)
SELECT
    v.company_id,
    l.id,
    v.id
FROM product_variants v
JOIN inventory_locations l
    ON l.company_id = v.company_id
   AND l.is_default = TRUE;

-- ------------------------------------------------------------
-- 8. STOCK MOVEMENTS
-- Permanent record of every stock change.
-- quantity_delta is positive for incoming stock
-- and negative for outgoing stock.
-- ------------------------------------------------------------

CREATE TABLE stock_movements (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    location_id BIGINT NOT NULL,
    variant_id BIGINT NOT NULL,
    movement_type VARCHAR(40) NOT NULL,
    quantity_delta NUMERIC(19, 4) NOT NULL,
    reference_type VARCHAR(40),
    reference_id VARCHAR(100),
    idempotency_key UUID,
    notes TEXT,
    created_by UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT fk_movements_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_movements_location
        FOREIGN KEY (location_id, company_id)
        REFERENCES inventory_locations(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_movements_variant
        FOREIGN KEY (variant_id, company_id)
        REFERENCES product_variants(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_movements_user
        FOREIGN KEY (created_by, company_id)
        REFERENCES users(id, company_id)
        ON DELETE SET NULL (created_by),

    CONSTRAINT chk_movements_type
        CHECK (
            movement_type IN (
                'opening_balance',
                'purchase_receipt',
                'sale',
                'sale_return',
                'purchase_return',
                'adjustment_in',
                'adjustment_out',
                'transfer_in',
                'transfer_out',
                'count_adjustment'
            )
        ),

    CONSTRAINT chk_movements_quantity
        CHECK (quantity_delta <> 0)
);

CREATE UNIQUE INDEX uq_movements_idempotency
ON stock_movements(company_id, idempotency_key)
WHERE idempotency_key IS NOT NULL;

CREATE INDEX idx_movements_variant_date
ON stock_movements(company_id, variant_id, created_at DESC);

CREATE INDEX idx_movements_location_date
ON stock_movements(company_id, location_id, created_at DESC);

CREATE INDEX idx_movements_reference
ON stock_movements(company_id, reference_type, reference_id);

-- ------------------------------------------------------------
-- 9. SUPPLIERS
-- ------------------------------------------------------------

CREATE TABLE suppliers (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    name VARCHAR(150) NOT NULL,
    contact_person VARCHAR(150),
    email VARCHAR(255),
    phone VARCHAR(50),
    address TEXT,
    tax_number VARCHAR(80),
    notes TEXT,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INTEGER NOT NULL DEFAULT 1,

    CONSTRAINT fk_suppliers_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT uq_suppliers_id_company
        UNIQUE (id, company_id),

    CONSTRAINT chk_suppliers_name
        CHECK (LENGTH(TRIM(name)) > 0),

    CONSTRAINT chk_suppliers_version
        CHECK (version > 0)
);

CREATE INDEX idx_suppliers_company
ON suppliers(company_id);

CREATE INDEX idx_suppliers_company_name
ON suppliers(company_id, LOWER(name));

-- ------------------------------------------------------------
-- 10. SUPPLIER PRODUCTS
-- Links suppliers to the variants they supply.
-- ------------------------------------------------------------

CREATE TABLE supplier_products (
    company_id UUID NOT NULL,
    supplier_id BIGINT NOT NULL,
    variant_id BIGINT NOT NULL,
    supplier_sku VARCHAR(80),
    last_cost_price NUMERIC(19, 4),
    preferred_supplier BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    PRIMARY KEY (supplier_id, variant_id),

    CONSTRAINT fk_supplier_products_supplier
        FOREIGN KEY (supplier_id, company_id)
        REFERENCES suppliers(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT fk_supplier_products_variant
        FOREIGN KEY (variant_id, company_id)
        REFERENCES product_variants(id, company_id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_supplier_products_cost
        CHECK (last_cost_price IS NULL OR last_cost_price >= 0)
);

CREATE INDEX idx_supplier_products_variant
ON supplier_products(company_id, variant_id);
