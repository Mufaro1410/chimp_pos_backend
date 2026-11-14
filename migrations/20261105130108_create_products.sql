-- Add migration script here
CREATE TABLE products (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

    company_id UUID NOT NULL,

    name VARCHAR(150) NOT NULL,

    price NUMERIC(19, 4) NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT fk_products_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_products_name_not_empty
        CHECK (LENGTH(TRIM(name)) > 0),

    CONSTRAINT chk_products_price_non_negative
        CHECK (price >= 0)
);

-- A product name must be unique within its company,
-- but different companies may use the same name.
CREATE UNIQUE INDEX uq_products_company_name
ON products (company_id, LOWER(name));

-- Efficient company-scoped product queries.
CREATE INDEX idx_products_company_id
ON products (company_id);

-- Efficient company-scoped alphabetical listings.
CREATE INDEX idx_products_company_name
ON products (company_id, name);