
-- ============================================================
-- CHIMPPOS
-- STAGE 13A: TRADING PARTIES
--
-- Customers, supplier account settings and sales integration.
--
-- Important:
-- - Existing suppliers are preserved.
-- - Existing sales remain anonymous (customer_id = NULL).
-- - Customer and supplier numbers are company-specific.
-- - Financial account tables are introduced in later migrations.
-- ============================================================


-- ============================================================
-- 1. CUSTOMER NUMBER SEQUENCES
-- ============================================================

CREATE TABLE customer_sequences (
    company_id UUID PRIMARY KEY,
    next_number BIGINT NOT NULL DEFAULT 1,

    CONSTRAINT fk_customer_sequences_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_customer_sequence_positive
        CHECK (next_number > 0)
);


-- Initialize sequences for existing companies.
INSERT INTO customer_sequences (company_id, next_number)
SELECT id, 1
FROM companies
ON CONFLICT (company_id) DO NOTHING;


-- ============================================================
-- 2. CUSTOMERS
-- ============================================================

CREATE TABLE customers (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

    company_id UUID NOT NULL,
    customer_number BIGINT NOT NULL,

    name VARCHAR(150) NOT NULL,
    email VARCHAR(255),
    phone VARCHAR(30),
    address TEXT,
    tax_number VARCHAR(80),

    credit_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    credit_limit NUMERIC(19,4) NOT NULL DEFAULT 0,
    payment_terms_days INTEGER NOT NULL DEFAULT 0,

    notes TEXT,

    is_active BOOLEAN NOT NULL DEFAULT TRUE,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    version INTEGER NOT NULL DEFAULT 1,

    CONSTRAINT uq_customers_id_company
        UNIQUE (id, company_id),

    CONSTRAINT uq_customers_number
        UNIQUE (company_id, customer_number),

    CONSTRAINT fk_customers_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_customers_name
        CHECK (LENGTH(TRIM(name)) > 0),

    CONSTRAINT chk_customers_credit_limit
        CHECK (credit_limit >= 0),

    CONSTRAINT chk_customers_payment_terms
        CHECK (payment_terms_days BETWEEN 0 AND 3650),

    CONSTRAINT chk_customers_version
        CHECK (version > 0),

    CONSTRAINT chk_customers_number
        CHECK (customer_number > 0)
);


-- Customer lookup and account listing.
CREATE INDEX idx_customers_company_name
    ON customers(company_id, name);

CREATE INDEX idx_customers_company_active
    ON customers(company_id, is_active);

CREATE INDEX idx_customers_company_phone
    ON customers(company_id, phone)
    WHERE phone IS NOT NULL;

CREATE INDEX idx_customers_company_email
    ON customers(company_id, LOWER(email))
    WHERE email IS NOT NULL;


-- ============================================================
-- 3. EXTEND EXISTING SUPPLIERS
-- ============================================================

ALTER TABLE suppliers
    ADD COLUMN supplier_number BIGINT,
    ADD COLUMN payment_terms_days INTEGER NOT NULL DEFAULT 0;


-- Assign stable supplier numbers to existing records.
-- Existing suppliers are numbered independently per company.
WITH numbered_suppliers AS (
    SELECT
        id,
        ROW_NUMBER() OVER (
            PARTITION BY company_id
            ORDER BY id
        ) AS assigned_number
    FROM suppliers
)
UPDATE suppliers s
SET supplier_number = ns.assigned_number
FROM numbered_suppliers ns
WHERE s.id = ns.id;


ALTER TABLE suppliers
    ALTER COLUMN supplier_number SET NOT NULL;


ALTER TABLE suppliers
    ADD CONSTRAINT chk_suppliers_number
        CHECK (supplier_number > 0),

    ADD CONSTRAINT chk_suppliers_payment_terms
        CHECK (payment_terms_days BETWEEN 0 AND 3650);


ALTER TABLE suppliers
    ADD CONSTRAINT uq_suppliers_number
        UNIQUE (company_id, supplier_number);


-- ============================================================
-- 4. SUPPLIER NUMBER SEQUENCES
-- ============================================================

CREATE TABLE supplier_sequences (
    company_id UUID PRIMARY KEY,
    next_number BIGINT NOT NULL DEFAULT 1,

    CONSTRAINT fk_supplier_sequences_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_supplier_sequence_positive
        CHECK (next_number > 0)
);


-- Initialize sequences after numbering existing suppliers.
-- Companies without suppliers start at number 1.
INSERT INTO supplier_sequences (company_id, next_number)
SELECT
    c.id,
    COALESCE(MAX(s.supplier_number), 0) + 1
FROM companies c
LEFT JOIN suppliers s
    ON s.company_id = c.id
GROUP BY c.id
ON CONFLICT (company_id)
DO UPDATE SET
    next_number = GREATEST(
        supplier_sequences.next_number,
        EXCLUDED.next_number
    );


-- ============================================================
-- 5. OPTIONAL CUSTOMER RELATIONSHIP ON SALES
-- ============================================================

ALTER TABLE sales
    ADD COLUMN customer_id BIGINT;


ALTER TABLE sales
    ADD CONSTRAINT fk_sales_customer
        FOREIGN KEY (customer_id, company_id)
        REFERENCES customers(id, company_id)
        ON DELETE RESTRICT;


-- Existing sales remain anonymous.
-- Customer sales can now be queried efficiently.
CREATE INDEX idx_sales_customer_date
    ON sales(company_id, customer_id, created_at DESC)
    WHERE customer_id IS NOT NULL;


-- ============================================================
-- END OF STAGE 13A
-- ============================================================
