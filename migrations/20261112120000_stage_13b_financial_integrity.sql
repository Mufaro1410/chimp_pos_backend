-- Stage 13B schema hardening.
-- Additive only: preserve existing rows and replace only the two weaker
-- supplier-invoice-line foreign keys with company-aware equivalents.

-- Stop safely if existing account data cannot satisfy the stronger invariants.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM sales s
        LEFT JOIN stage13b_sale_currency_approvals a
          ON a.company_id=s.company_id AND a.sale_id=s.id
        WHERE a.approved_currency IS NULL OR a.approved_by IS NULL OR a.approved_at IS NULL
           OR length(trim(COALESCE(a.evidence_reference, ''))) = 0
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: historical sale currency approval is unresolved';
    END IF;

    IF EXISTS (
        SELECT 1 FROM sales s
        JOIN stage13b_sale_currency_approvals a
          ON a.company_id=s.company_id AND a.sale_id=s.id
        JOIN companies c ON c.id=s.company_id
        WHERE a.approved_currency <> c.default_currency
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: non-base historical sales require an approved multi-currency receivable design';
    END IF;

    IF EXISTS (
        SELECT 1 FROM customer_payments p
        LEFT JOIN stage13b_customer_payment_currency_approvals a
          ON a.company_id=p.company_id AND a.payment_id=p.id AND a.customer_id=p.customer_id
        WHERE a.approved_currency IS NULL OR a.approved_by IS NULL OR a.approved_at IS NULL
           OR length(trim(COALESCE(a.evidence_reference, ''))) = 0
    ) OR EXISTS (
        SELECT 1 FROM customer_credit_notes n
        LEFT JOIN stage13b_customer_credit_note_currency_approvals a
          ON a.company_id=n.company_id AND a.credit_note_id=n.id AND a.customer_id=n.customer_id
        WHERE a.approved_currency IS NULL OR a.approved_by IS NULL OR a.approved_at IS NULL
           OR length(trim(COALESCE(a.evidence_reference, ''))) = 0
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: historical customer account currency approval is unresolved';
    END IF;

    IF EXISTS (
        SELECT 1 FROM customer_payments p
        JOIN stage13b_customer_payment_currency_approvals a
          ON a.company_id=p.company_id AND a.payment_id=p.id AND a.customer_id=p.customer_id
        JOIN companies c ON c.id=p.company_id
        WHERE a.approved_currency <> c.default_currency
    ) OR EXISTS (
        SELECT 1 FROM customer_credit_notes n
        JOIN stage13b_customer_credit_note_currency_approvals a
          ON a.company_id=n.company_id AND a.credit_note_id=n.id AND a.customer_id=n.customer_id
        JOIN companies c ON c.id=n.company_id
        WHERE a.approved_currency <> c.default_currency
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: customer account currency must be reconciled to the supported company base currency';
    END IF;

    IF EXISTS (
        SELECT 1 FROM supplier_payments p
        LEFT JOIN stage13b_supplier_payment_currency_approvals a
          ON a.company_id=p.company_id AND a.payment_id=p.id AND a.supplier_id=p.supplier_id
        WHERE a.approved_currency IS NULL OR a.approved_by IS NULL OR a.approved_at IS NULL
           OR length(trim(COALESCE(a.evidence_reference, ''))) = 0
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: supplier payment currency approval is unresolved';
    END IF;

    IF EXISTS (
        SELECT 1 FROM supplier_payment_allocations a
        JOIN supplier_payments p ON p.id=a.payment_id AND p.company_id=a.company_id
        JOIN supplier_invoices i ON i.id=a.invoice_id AND i.company_id=a.company_id
        JOIN stage13b_supplier_payment_currency_approvals c
          ON c.company_id=p.company_id AND c.payment_id=p.id AND c.supplier_id=p.supplier_id
        WHERE c.approved_currency IS DISTINCT FROM i.currency_code
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: approved supplier payment currency conflicts with an allocated invoice';
    END IF;

    IF EXISTS (
        SELECT 1 FROM sale_refunds r
        LEFT JOIN stage13b_cash_event_reconciliations a
          ON a.company_id=r.company_id AND a.event_type='sale_refund' AND a.event_id=r.id
        WHERE r.payment_method='cash' AND r.cash_session_id IS NULL
          AND (a.disposition IS DISTINCT FROM 'outside_session_exception'
               OR a.approved_by IS NULL OR a.approved_at IS NULL
               OR length(trim(COALESCE(a.evidence_reference, ''))) = 0
               OR length(trim(COALESCE(a.approval_notes, ''))) = 0)
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: cash refund session attribution is unresolved';
    END IF;

    IF EXISTS (
        SELECT 1 FROM stage13b_cash_event_reconciliations a
        LEFT JOIN sale_refunds r ON a.event_type='sale_refund'
          AND r.company_id=a.company_id AND r.id=a.event_id
        LEFT JOIN sale_payments p ON a.event_type='sale_payment'
          AND p.company_id=a.company_id AND p.id=a.event_id
        WHERE a.disposition='verified_session'
          AND (a.approved_by IS NULL OR a.approved_at IS NULL
            OR length(trim(COALESCE(a.evidence_reference, ''))) = 0
            OR (a.event_type='sale_refund' AND (r.id IS NULL OR r.cash_session_id IS DISTINCT FROM a.approved_session_id))
            OR (a.event_type='sale_payment' AND (p.id IS NULL OR p.cash_session_id IS DISTINCT FROM a.approved_session_id)))
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: verified cash event session does not match source record';
    END IF;

    IF EXISTS (
        SELECT 1 FROM sale_payments p
        LEFT JOIN stage13b_cash_event_reconciliations a
          ON a.company_id=p.company_id AND a.event_type='sale_payment' AND a.event_id=p.id
        WHERE p.payment_method='cash' AND p.cash_session_id IS NULL
          AND (a.disposition IS DISTINCT FROM 'outside_session_exception'
               OR a.approved_by IS NULL OR a.approved_at IS NULL
               OR length(trim(COALESCE(a.evidence_reference, ''))) = 0
               OR length(trim(COALESCE(a.approval_notes, ''))) = 0)
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: cash sale payment session attribution is unresolved';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM supplier_invoice_items x
        JOIN supplier_invoices i ON i.id = x.invoice_id
        LEFT JOIN purchase_order_items poi ON poi.id = x.purchase_order_item_id
        LEFT JOIN goods_receipt_items gri ON gri.id = x.goods_receipt_item_id
        WHERE i.company_id <> x.company_id
           OR (x.purchase_order_item_id IS NOT NULL AND poi.company_id <> x.company_id)
           OR (x.goods_receipt_item_id IS NOT NULL AND gri.company_id <> x.company_id)
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: supplier invoice item has a cross-company reference';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM supplier_payment_allocations a
        JOIN supplier_payments p ON p.id = a.payment_id
        JOIN supplier_invoices i ON i.id = a.invoice_id
        WHERE p.supplier_id <> i.supplier_id
           OR p.company_id <> i.company_id
    ) OR EXISTS (
        SELECT 1
        FROM supplier_payment_allocations a
        JOIN supplier_invoices i ON i.id = a.invoice_id
        GROUP BY a.payment_id
        HAVING COUNT(DISTINCT i.currency_code) > 1
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: supplier allocation party mismatch';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM customer_payment_allocations a
        JOIN customer_payments p ON p.id = a.payment_id
        JOIN sales s ON s.id = a.sale_id
        WHERE p.customer_id IS DISTINCT FROM s.customer_id OR s.customer_id IS NULL
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: customer allocation party mismatch';
    END IF;

    IF EXISTS (
        SELECT 1 FROM customer_payment_allocations a JOIN sales s ON s.id=a.sale_id
        WHERE s.status <> 'completed'
    ) OR EXISTS (
        SELECT 1 FROM supplier_payment_allocations a JOIN supplier_invoices i ON i.id=a.invoice_id
        WHERE i.status <> 'posted'
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: allocation points to an ineligible document';
    END IF;

    IF EXISTS (
        SELECT 1 FROM supplier_invoices i
        JOIN purchase_orders po ON po.id = i.purchase_order_id
        WHERE i.supplier_id <> po.supplier_id
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: invoice and purchase order suppliers differ';
    END IF;

    IF EXISTS (
        SELECT 1 FROM customer_credit_notes n
        JOIN sales s ON s.id = n.sale_id
        WHERE n.customer_id IS DISTINCT FROM s.customer_id OR s.customer_id IS NULL
    ) OR EXISTS (
        SELECT 1 FROM supplier_credit_notes n
        JOIN supplier_invoices i ON i.id = n.invoice_id
        WHERE n.supplier_id <> i.supplier_id
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: credit note party mismatch';
    END IF;

    IF EXISTS (
        SELECT 1 FROM customer_payment_allocations a
        JOIN customer_payments p ON p.id = a.payment_id
        GROUP BY p.id, p.amount HAVING SUM(a.amount) > p.amount
    ) OR EXISTS (
        SELECT 1 FROM supplier_payment_allocations a
        JOIN supplier_payments p ON p.id = a.payment_id
        GROUP BY p.id, p.amount HAVING SUM(a.amount) > p.amount
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: existing payment is over-allocated';
    END IF;

    IF EXISTS (
        SELECT 1 FROM sales s
        WHERE s.status='completed'
          AND COALESCE((SELECT SUM(a.amount) FROM customer_payment_allocations a WHERE a.sale_id=s.id AND a.company_id=s.company_id),0)
            + COALESCE((SELECT SUM(sp.amount) FROM sale_payments sp WHERE sp.sale_id=s.id AND sp.company_id=s.company_id),0)
            + COALESCE((SELECT SUM(cn.amount) FROM customer_credit_notes cn WHERE cn.sale_id=s.id AND cn.company_id=s.company_id),0)
            > s.total_amount
    ) OR EXISTS (
        SELECT 1 FROM supplier_invoices i
        WHERE COALESCE((SELECT SUM(a.amount) FROM supplier_payment_allocations a WHERE a.invoice_id=i.id AND a.company_id=i.company_id),0)
            + COALESCE((SELECT SUM(n.amount) FROM supplier_credit_notes n WHERE n.invoice_id=i.id AND n.company_id=i.company_id),0)
            > i.total_amount
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: existing allocations and credits exceed document totals';
    END IF;

    IF EXISTS (
        SELECT 1 FROM customer_payments
        WHERE payment_method='cash' AND cash_session_id IS NULL
    ) OR EXISTS (
        SELECT 1 FROM supplier_payments
        WHERE payment_method='cash' AND cash_session_id IS NULL
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: existing cash account event has no cash session';
    END IF;

    IF EXISTS (
        SELECT 1 FROM supplier_invoice_items x
        JOIN supplier_invoices i ON i.id=x.invoice_id AND i.company_id=x.company_id
        LEFT JOIN purchase_order_items poi ON poi.id=x.purchase_order_item_id AND poi.company_id=x.company_id
        LEFT JOIN goods_receipt_items gri ON gri.id=x.goods_receipt_item_id AND gri.company_id=x.company_id
        LEFT JOIN purchase_order_items receipt_poi ON receipt_poi.id=gri.purchase_order_item_id AND receipt_poi.company_id=gri.company_id
        WHERE (x.purchase_order_item_id IS NOT NULL AND i.purchase_order_id IS DISTINCT FROM poi.purchase_order_id)
           OR (x.goods_receipt_item_id IS NOT NULL AND (i.purchase_order_id IS DISTINCT FROM receipt_poi.purchase_order_id
               OR (x.purchase_order_item_id IS NOT NULL AND x.purchase_order_item_id IS DISTINCT FROM gri.purchase_order_item_id)))
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: invoice line source items do not match invoice purchase order';
    END IF;

    IF EXISTS (
        SELECT 1 FROM supplier_invoice_items
        WHERE line_total <> ROUND(quantity * unit_cost + tax_amount - discount_amount, 4)
    ) OR EXISTS (
        SELECT 1 FROM supplier_invoices i
        WHERE i.discount_total > i.subtotal + i.tax_total
           OR (i.status='posted' AND (
                NOT EXISTS (SELECT 1 FROM supplier_invoice_items x WHERE x.invoice_id=i.id AND x.company_id=i.company_id)
                OR i.subtotal + i.tax_total - i.discount_total <>
                   (SELECT COALESCE(SUM(x.line_total),0) FROM supplier_invoice_items x WHERE x.invoice_id=i.id AND x.company_id=i.company_id)
           ))
    ) THEN
        RAISE EXCEPTION 'Stage 13B hardening stopped: supplier invoice amounts do not reconcile';
    END IF;

END;
$$;

-- Explicit transaction currency. Historical sale currency comes only from an
-- approved, evidence-backed reconciliation row, never from today's company default.
ALTER TABLE companies
    ADD CONSTRAINT uq_companies_id_default_currency UNIQUE (id, default_currency),
    ADD CONSTRAINT chk_companies_default_currency_format
        CHECK (default_currency ~ '^[A-Z]{3}$');

ALTER TABLE sales ADD COLUMN currency_code CHAR(3);
UPDATE sales s
SET currency_code = a.approved_currency
FROM stage13b_sale_currency_approvals a
WHERE a.company_id=s.company_id AND a.sale_id=s.id
  AND a.approved_by IS NOT NULL AND a.approved_at IS NOT NULL;
ALTER TABLE sales ALTER COLUMN currency_code SET NOT NULL;
ALTER TABLE sales
    ADD CONSTRAINT uq_sales_id_company_customer_currency
        UNIQUE (id, company_id, customer_id, currency_code),
    ADD CONSTRAINT uq_sales_id_company_currency
        UNIQUE (id, company_id, currency_code),
    ADD CONSTRAINT fk_sales_company_currency
        FOREIGN KEY (company_id, currency_code)
        REFERENCES companies (id, default_currency)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

ALTER TABLE customers ADD COLUMN currency_code CHAR(3);
UPDATE customers c
SET currency_code = co.default_currency
FROM companies co
WHERE co.id = c.company_id;
ALTER TABLE customers ALTER COLUMN currency_code SET NOT NULL;
ALTER TABLE customers
    ADD CONSTRAINT uq_customers_id_company_currency
        UNIQUE (id, company_id, currency_code),
    ADD CONSTRAINT fk_customers_company_currency
        FOREIGN KEY (company_id, currency_code)
        REFERENCES companies (id, default_currency)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

ALTER TABLE sale_payments ADD COLUMN currency_code CHAR(3);
UPDATE sale_payments p
SET currency_code = s.currency_code
FROM sales s
WHERE s.id = p.sale_id AND s.company_id = p.company_id;
ALTER TABLE sale_payments ALTER COLUMN currency_code SET NOT NULL;
ALTER TABLE sale_payments
    DROP CONSTRAINT fk_sale_payments_sale,
    ADD CONSTRAINT fk_sale_payments_sale_currency
        FOREIGN KEY (sale_id, company_id, currency_code)
        REFERENCES sales (id, company_id, currency_code)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

ALTER TABLE sale_refunds ADD COLUMN currency_code CHAR(3);
UPDATE sale_refunds r
SET currency_code=s.currency_code
FROM sales s
WHERE s.id=r.sale_id AND s.company_id=r.company_id;
ALTER TABLE sale_refunds ALTER COLUMN currency_code SET NOT NULL;
ALTER TABLE sale_refunds
    ADD CONSTRAINT chk_sale_refunds_currency_format
        CHECK (currency_code ~ '^[A-Z]{3}$'),
    ADD CONSTRAINT fk_sale_refunds_sale_currency
        FOREIGN KEY (sale_id, company_id, currency_code)
        REFERENCES sales (id, company_id, currency_code)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

ALTER TABLE customer_payments ADD COLUMN currency_code CHAR(3);
UPDATE customer_payments p
SET currency_code = a.approved_currency
FROM stage13b_customer_payment_currency_approvals a
WHERE a.company_id=p.company_id AND a.payment_id=p.id AND a.customer_id=p.customer_id
  AND a.approved_by IS NOT NULL AND a.approved_at IS NOT NULL;
ALTER TABLE customer_payments ALTER COLUMN currency_code SET NOT NULL;
ALTER TABLE customer_payments
    DROP CONSTRAINT fk_customer_payment_customer,
    ADD CONSTRAINT uq_customer_payments_id_company_customer_currency
        UNIQUE (id, company_id, customer_id, currency_code),
    ADD CONSTRAINT fk_customer_payments_company_currency
        FOREIGN KEY (company_id, currency_code)
        REFERENCES companies (id, default_currency)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT fk_customer_payments_customer_currency
        FOREIGN KEY (customer_id, company_id, currency_code)
        REFERENCES customers (id, company_id, currency_code)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT chk_customer_payments_currency_format
        CHECK (currency_code ~ '^[A-Z]{3}$');

ALTER TABLE customer_credit_notes ADD COLUMN currency_code CHAR(3);
ALTER TABLE customer_credit_notes ADD COLUMN request_fingerprint TEXT;
UPDATE customer_credit_notes n
SET currency_code = a.approved_currency
FROM stage13b_customer_credit_note_currency_approvals a
WHERE a.company_id=n.company_id AND a.credit_note_id=n.id AND a.customer_id=n.customer_id
  AND a.approved_by IS NOT NULL AND a.approved_at IS NOT NULL;
UPDATE customer_credit_notes n
SET request_fingerprint = jsonb_build_array(n.company_id::text,n.customer_id::text,
    n.sale_id,n.amount::text,n.reason)::text;
ALTER TABLE customer_credit_notes ALTER COLUMN currency_code SET NOT NULL;
ALTER TABLE customer_credit_notes ALTER COLUMN request_fingerprint SET NOT NULL;
ALTER TABLE customer_credit_notes
    DROP CONSTRAINT customer_credit_notes_sale_id_company_id_fkey,
    ADD CONSTRAINT fk_customer_credit_notes_company_currency
        FOREIGN KEY (company_id, currency_code)
        REFERENCES companies (id, default_currency)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT chk_customer_credit_notes_currency_format
        CHECK (currency_code ~ '^[A-Z]{3}$'),
    ADD CONSTRAINT chk_customer_credit_notes_fingerprint
        CHECK (length(trim(request_fingerprint)) > 0),
    ADD CONSTRAINT fk_customer_credit_notes_sale_party_currency
        FOREIGN KEY (sale_id, company_id, customer_id, currency_code)
        REFERENCES sales (id, company_id, customer_id, currency_code)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

-- Supplier invoices can use a foreign currency. Their payments, allocations and
-- credits carry that same explicit currency and account views stay currency-wise.
ALTER TABLE supplier_invoices
    ADD CONSTRAINT chk_supplier_invoices_currency_format
        CHECK (currency_code ~ '^[A-Z]{3}$'),
    ADD CONSTRAINT uq_supplier_invoices_id_company_supplier_currency
        UNIQUE (id, company_id, supplier_id, currency_code);

ALTER TABLE supplier_payments ADD COLUMN currency_code CHAR(3);
UPDATE supplier_payments p
SET currency_code = a.approved_currency
FROM stage13b_supplier_payment_currency_approvals a
WHERE a.company_id=p.company_id AND a.payment_id=p.id AND a.supplier_id=p.supplier_id
  AND a.approved_by IS NOT NULL AND a.approved_at IS NOT NULL;
ALTER TABLE supplier_payments ALTER COLUMN currency_code SET NOT NULL;
ALTER TABLE supplier_payments
    ADD CONSTRAINT chk_supplier_payments_currency_format
        CHECK (currency_code ~ '^[A-Z]{3}$'),
    ADD CONSTRAINT uq_supplier_payments_id_company_supplier_currency
        UNIQUE (id, company_id, supplier_id, currency_code);

ALTER TABLE supplier_credit_notes ADD COLUMN currency_code CHAR(3);
ALTER TABLE supplier_credit_notes ADD COLUMN request_fingerprint TEXT;
UPDATE supplier_credit_notes n
SET currency_code = i.currency_code
FROM supplier_invoices i
WHERE i.id = n.invoice_id AND i.company_id = n.company_id;
UPDATE supplier_credit_notes n
SET request_fingerprint = jsonb_build_array(n.company_id::text,n.supplier_id::text,
    n.invoice_id,n.amount::text,n.reason)::text;
ALTER TABLE supplier_credit_notes ALTER COLUMN currency_code SET NOT NULL;
ALTER TABLE supplier_credit_notes ALTER COLUMN request_fingerprint SET NOT NULL;
ALTER TABLE supplier_credit_notes
    DROP CONSTRAINT supplier_credit_notes_supplier_id_company_id_fkey,
    DROP CONSTRAINT supplier_credit_notes_invoice_id_company_id_fkey,
    ADD CONSTRAINT chk_supplier_credit_notes_currency_format
        CHECK (currency_code ~ '^[A-Z]{3}$'),
    ADD CONSTRAINT chk_supplier_credit_notes_fingerprint
        CHECK (length(trim(request_fingerprint)) > 0),
    ADD CONSTRAINT fk_supplier_credit_notes_invoice_party_currency
        FOREIGN KEY (invoice_id, company_id, supplier_id, currency_code)
        REFERENCES supplier_invoices (id, company_id, supplier_id, currency_code)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

-- Company-aware line references. Drop only the narrower foreign keys being
-- replaced; their rows and referenced tables are untouched.
ALTER TABLE goods_receipt_items
    ADD CONSTRAINT uq_goods_receipt_items_id_company UNIQUE (id, company_id);
ALTER TABLE purchase_orders
    ADD CONSTRAINT uq_purchase_orders_id_company_supplier UNIQUE (id, company_id, supplier_id);

ALTER TABLE supplier_invoices
    DROP CONSTRAINT supplier_invoices_purchase_order_id_company_id_fkey,
    ADD CONSTRAINT fk_supplier_invoices_order_supplier
        FOREIGN KEY (purchase_order_id, company_id, supplier_id)
        REFERENCES purchase_orders (id, company_id, supplier_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

ALTER TABLE supplier_invoice_items
    DROP CONSTRAINT supplier_invoice_items_purchase_order_item_id_fkey,
    DROP CONSTRAINT supplier_invoice_items_goods_receipt_item_id_fkey,
    ADD CONSTRAINT fk_supplier_invoice_items_purchase_item_company
        FOREIGN KEY (purchase_order_item_id, company_id)
        REFERENCES purchase_order_items (id, company_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT fk_supplier_invoice_items_receipt_item_company
        FOREIGN KEY (goods_receipt_item_id, company_id)
        REFERENCES goods_receipt_items (id, company_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

ALTER TABLE customer_payment_allocations ADD COLUMN customer_id BIGINT;
ALTER TABLE customer_payment_allocations ADD COLUMN currency_code CHAR(3);
UPDATE customer_payment_allocations a
SET customer_id = p.customer_id,
    currency_code = p.currency_code
FROM customer_payments p
WHERE p.id = a.payment_id AND p.company_id = a.company_id;
ALTER TABLE customer_payment_allocations
    ALTER COLUMN customer_id SET NOT NULL,
    ALTER COLUMN currency_code SET NOT NULL,
    DROP CONSTRAINT customer_payment_allocations_payment_id_company_id_fkey,
    DROP CONSTRAINT customer_payment_allocations_sale_id_company_id_fkey,
    ADD CONSTRAINT fk_customer_allocations_payment_party_currency
        FOREIGN KEY (payment_id, company_id, customer_id, currency_code)
        REFERENCES customer_payments (id, company_id, customer_id, currency_code)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT fk_customer_allocations_sale_party_currency
        FOREIGN KEY (sale_id, company_id, customer_id, currency_code)
        REFERENCES sales (id, company_id, customer_id, currency_code)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

ALTER TABLE supplier_payment_allocations ADD COLUMN supplier_id BIGINT;
ALTER TABLE supplier_payment_allocations ADD COLUMN currency_code CHAR(3);
UPDATE supplier_payment_allocations a
SET supplier_id = p.supplier_id,
    currency_code = p.currency_code
FROM supplier_payments p
WHERE p.id = a.payment_id AND p.company_id = a.company_id;
ALTER TABLE supplier_payment_allocations
    ALTER COLUMN supplier_id SET NOT NULL,
    ALTER COLUMN currency_code SET NOT NULL,
    DROP CONSTRAINT supplier_payment_allocations_payment_id_company_id_fkey,
    DROP CONSTRAINT supplier_payment_allocations_invoice_id_company_id_fkey,
    ADD CONSTRAINT fk_supplier_allocations_payment_party_currency
        FOREIGN KEY (payment_id, company_id, supplier_id, currency_code)
        REFERENCES supplier_payments (id, company_id, supplier_id, currency_code)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    ADD CONSTRAINT fk_supplier_allocations_invoice_party_currency
        FOREIGN KEY (invoice_id, company_id, supplier_id, currency_code)
        REFERENCES supplier_invoices (id, company_id, supplier_id, currency_code)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

ALTER TABLE customer_payment_allocations
    ADD CONSTRAINT uq_customer_allocation_reversal_source
        UNIQUE (id, company_id, payment_id, sale_id, customer_id, currency_code);
ALTER TABLE supplier_payment_allocations
    ADD CONSTRAINT uq_supplier_allocation_reversal_source
        UNIQUE (id, company_id, payment_id, invoice_id, supplier_id, currency_code);
ALTER TABLE customer_credit_notes
    ADD CONSTRAINT uq_customer_credit_reversal_source
        UNIQUE (id, company_id, customer_id, currency_code);
ALTER TABLE supplier_credit_notes
    ADD CONSTRAINT uq_supplier_credit_reversal_source
        UNIQUE (id, company_id, supplier_id, currency_code);

-- Reversal tables contain positive amounts and immutable source snapshots. Their
-- unique company/idempotency keys make retries deterministic.
CREATE TABLE customer_allocation_reversals (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    allocation_id BIGINT NOT NULL,
    payment_id BIGINT NOT NULL,
    sale_id BIGINT NOT NULL,
    customer_id BIGINT NOT NULL,
    currency_code CHAR(3) NOT NULL,
    amount NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    reason TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    idempotency_key UUID NOT NULL,
    request_fingerprint TEXT NOT NULL CHECK (length(trim(request_fingerprint)) > 0),
    reversed_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    FOREIGN KEY (allocation_id, company_id, payment_id, sale_id, customer_id, currency_code)
        REFERENCES customer_payment_allocations(id, company_id, payment_id, sale_id, customer_id, currency_code)
        ON DELETE RESTRICT,
    FOREIGN KEY (reversed_by, company_id) REFERENCES users(id, company_id) ON DELETE RESTRICT,
    UNIQUE (company_id, idempotency_key)
);

CREATE TABLE supplier_allocation_reversals (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    allocation_id BIGINT NOT NULL,
    payment_id BIGINT NOT NULL,
    invoice_id BIGINT NOT NULL,
    supplier_id BIGINT NOT NULL,
    currency_code CHAR(3) NOT NULL,
    amount NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    reason TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    idempotency_key UUID NOT NULL,
    request_fingerprint TEXT NOT NULL CHECK (length(trim(request_fingerprint)) > 0),
    reversed_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    FOREIGN KEY (allocation_id, company_id, payment_id, invoice_id, supplier_id, currency_code)
        REFERENCES supplier_payment_allocations(id, company_id, payment_id, invoice_id, supplier_id, currency_code)
        ON DELETE RESTRICT,
    FOREIGN KEY (reversed_by, company_id) REFERENCES users(id, company_id) ON DELETE RESTRICT,
    UNIQUE (company_id, idempotency_key)
);

CREATE TABLE customer_payment_reversals (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    payment_id BIGINT NOT NULL,
    customer_id BIGINT NOT NULL,
    currency_code CHAR(3) NOT NULL,
    amount NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    payment_method VARCHAR(30) NOT NULL CHECK (payment_method IN ('cash','card','mobile_money','other')),
    cash_session_id BIGINT,
    reason TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    idempotency_key UUID NOT NULL,
    request_fingerprint TEXT NOT NULL CHECK (length(trim(request_fingerprint)) > 0),
    reversed_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    FOREIGN KEY (payment_id, company_id, customer_id, currency_code)
        REFERENCES customer_payments(id, company_id, customer_id, currency_code) ON DELETE RESTRICT,
    FOREIGN KEY (cash_session_id, company_id)
        REFERENCES cash_register_sessions(id, company_id) ON DELETE RESTRICT,
    FOREIGN KEY (reversed_by, company_id) REFERENCES users(id, company_id) ON DELETE RESTRICT,
    UNIQUE (company_id, idempotency_key),
    CHECK ((payment_method='cash' AND cash_session_id IS NOT NULL)
        OR (payment_method<>'cash' AND cash_session_id IS NULL))
);

CREATE TABLE supplier_payment_reversals (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    payment_id BIGINT NOT NULL,
    supplier_id BIGINT NOT NULL,
    currency_code CHAR(3) NOT NULL,
    amount NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    payment_method VARCHAR(30) NOT NULL CHECK (payment_method IN ('cash','card','mobile_money','other')),
    cash_session_id BIGINT,
    reason TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    idempotency_key UUID NOT NULL,
    request_fingerprint TEXT NOT NULL CHECK (length(trim(request_fingerprint)) > 0),
    reversed_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    FOREIGN KEY (payment_id, company_id, supplier_id, currency_code)
        REFERENCES supplier_payments(id, company_id, supplier_id, currency_code) ON DELETE RESTRICT,
    FOREIGN KEY (cash_session_id, company_id)
        REFERENCES cash_register_sessions(id, company_id) ON DELETE RESTRICT,
    FOREIGN KEY (reversed_by, company_id) REFERENCES users(id, company_id) ON DELETE RESTRICT,
    UNIQUE (company_id, idempotency_key),
    CHECK ((payment_method='cash' AND cash_session_id IS NOT NULL)
        OR (payment_method<>'cash' AND cash_session_id IS NULL))
);

CREATE TABLE customer_credit_note_reversals (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    credit_note_id BIGINT NOT NULL,
    customer_id BIGINT NOT NULL,
    currency_code CHAR(3) NOT NULL,
    amount NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    reason TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    idempotency_key UUID NOT NULL,
    request_fingerprint TEXT NOT NULL CHECK (length(trim(request_fingerprint)) > 0),
    reversed_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    FOREIGN KEY (credit_note_id, company_id, customer_id, currency_code)
        REFERENCES customer_credit_notes(id, company_id, customer_id, currency_code) ON DELETE RESTRICT,
    FOREIGN KEY (reversed_by, company_id) REFERENCES users(id, company_id) ON DELETE RESTRICT,
    UNIQUE (company_id, idempotency_key)
);

CREATE TABLE supplier_credit_note_reversals (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    company_id UUID NOT NULL,
    credit_note_id BIGINT NOT NULL,
    supplier_id BIGINT NOT NULL,
    currency_code CHAR(3) NOT NULL,
    amount NUMERIC(19,4) NOT NULL CHECK (amount > 0),
    reason TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    idempotency_key UUID NOT NULL,
    request_fingerprint TEXT NOT NULL CHECK (length(trim(request_fingerprint)) > 0),
    reversed_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    FOREIGN KEY (credit_note_id, company_id, supplier_id, currency_code)
        REFERENCES supplier_credit_notes(id, company_id, supplier_id, currency_code) ON DELETE RESTRICT,
    FOREIGN KEY (reversed_by, company_id) REFERENCES users(id, company_id) ON DELETE RESTRICT,
    UNIQUE (company_id, idempotency_key)
);

-- Invoice line math is exact to the existing four-decimal money convention.
ALTER TABLE supplier_invoice_items
    ADD CONSTRAINT chk_supplier_invoice_item_line_total
        CHECK (line_total = ROUND(quantity * unit_cost + tax_amount - discount_amount, 4));
ALTER TABLE supplier_invoices
    ADD CONSTRAINT chk_supplier_invoice_discount_ceiling
        CHECK (discount_total <= subtotal + tax_total);

-- Cash account events must link to an open session. The row lock makes the
-- posting serialize with cash-session close; cash stays in source tables and is
-- never duplicated as a manual cash_movement.
ALTER TABLE customer_payments
    DROP CONSTRAINT chk_customer_payment_cash_session,
    ADD CONSTRAINT chk_customer_payment_cash_session_required
        CHECK ((payment_method = 'cash' AND cash_session_id IS NOT NULL)
            OR (payment_method <> 'cash' AND cash_session_id IS NULL));
ALTER TABLE supplier_payments
    DROP CONSTRAINT supplier_payments_check,
    ADD CONSTRAINT chk_supplier_payment_cash_session_required
        CHECK ((payment_method = 'cash' AND cash_session_id IS NOT NULL)
            OR (payment_method <> 'cash' AND cash_session_id IS NULL));

CREATE FUNCTION enforce_account_payment_open_cash_session()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    session_status VARCHAR(20);
    base_currency CHAR(3);
BEGIN
    IF NEW.payment_method = 'cash' THEN
        SELECT default_currency INTO base_currency FROM companies
          WHERE id=NEW.company_id;
        IF NEW.currency_code <> base_currency THEN
            RAISE EXCEPTION 'cash account payment currency must match company cash currency'
                USING ERRCODE='23514';
        END IF;
        SELECT status INTO session_status
        FROM cash_register_sessions
        WHERE id = NEW.cash_session_id AND company_id = NEW.company_id
        FOR UPDATE;
        IF session_status IS DISTINCT FROM 'open' THEN
            RAISE EXCEPTION 'cash account payment requires an open company cash session'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION enforce_customer_allocation_reversal_limit()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE original_amount NUMERIC(19,4); already_reversed NUMERIC(19,4);
BEGIN
    PERFORM 1 FROM customer_payments WHERE id=NEW.payment_id AND company_id=NEW.company_id FOR UPDATE;
    PERFORM 1 FROM sales WHERE id=NEW.sale_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT amount INTO original_amount FROM customer_payment_allocations
      WHERE id=NEW.allocation_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT COALESCE(SUM(amount),0) INTO already_reversed
      FROM customer_allocation_reversals WHERE allocation_id=NEW.allocation_id AND company_id=NEW.company_id;
    IF already_reversed + NEW.amount > original_amount THEN
        RAISE EXCEPTION 'customer allocation reversals exceed original allocation' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION enforce_supplier_allocation_reversal_limit()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE original_amount NUMERIC(19,4); already_reversed NUMERIC(19,4);
BEGIN
    PERFORM 1 FROM supplier_payments WHERE id=NEW.payment_id AND company_id=NEW.company_id FOR UPDATE;
    PERFORM 1 FROM supplier_invoices WHERE id=NEW.invoice_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT amount INTO original_amount FROM supplier_payment_allocations
      WHERE id=NEW.allocation_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT COALESCE(SUM(amount),0) INTO already_reversed
      FROM supplier_allocation_reversals WHERE allocation_id=NEW.allocation_id AND company_id=NEW.company_id;
    IF already_reversed + NEW.amount > original_amount THEN
        RAISE EXCEPTION 'supplier allocation reversals exceed original allocation' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION enforce_customer_payment_reversal_limit()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE original_amount NUMERIC(19,4); active_allocations NUMERIC(19,4); already_reversed NUMERIC(19,4);
BEGIN
    SELECT amount INTO original_amount FROM customer_payments
      WHERE id=NEW.payment_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT COALESCE((SELECT SUM(a.amount) FROM customer_payment_allocations a
                     WHERE a.payment_id=NEW.payment_id AND a.company_id=NEW.company_id),0)
         - COALESCE((SELECT SUM(r.amount) FROM customer_allocation_reversals r
                     WHERE r.payment_id=NEW.payment_id AND r.company_id=NEW.company_id),0)
      INTO active_allocations;
    SELECT COALESCE(SUM(amount),0) INTO already_reversed
      FROM customer_payment_reversals WHERE payment_id=NEW.payment_id AND company_id=NEW.company_id;
    IF already_reversed + NEW.amount + active_allocations > original_amount THEN
        RAISE EXCEPTION 'customer payment reversal exceeds unallocated and reversed allocation amount' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION enforce_supplier_payment_reversal_limit()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE original_amount NUMERIC(19,4); active_allocations NUMERIC(19,4); already_reversed NUMERIC(19,4);
BEGIN
    SELECT amount INTO original_amount FROM supplier_payments
      WHERE id=NEW.payment_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT COALESCE((SELECT SUM(a.amount) FROM supplier_payment_allocations a
                     WHERE a.payment_id=NEW.payment_id AND a.company_id=NEW.company_id),0)
         - COALESCE((SELECT SUM(r.amount) FROM supplier_allocation_reversals r
                     WHERE r.payment_id=NEW.payment_id AND r.company_id=NEW.company_id),0)
      INTO active_allocations;
    SELECT COALESCE(SUM(amount),0) INTO already_reversed
      FROM supplier_payment_reversals WHERE payment_id=NEW.payment_id AND company_id=NEW.company_id;
    IF already_reversed + NEW.amount + active_allocations > original_amount THEN
        RAISE EXCEPTION 'supplier payment reversal exceeds unallocated and reversed allocation amount' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION enforce_customer_credit_note_reversal_limit()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE original_amount NUMERIC(19,4); already_reversed NUMERIC(19,4); source_sale BIGINT;
BEGIN
    PERFORM 1 FROM customers WHERE id=NEW.customer_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT sale_id INTO source_sale FROM customer_credit_notes
      WHERE id=NEW.credit_note_id AND company_id=NEW.company_id;
    IF source_sale IS NOT NULL THEN
        PERFORM 1 FROM sales WHERE id=source_sale AND company_id=NEW.company_id FOR UPDATE;
    END IF;
    SELECT amount INTO original_amount FROM customer_credit_notes
      WHERE id=NEW.credit_note_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT COALESCE(SUM(amount),0) INTO already_reversed FROM customer_credit_note_reversals
      WHERE credit_note_id=NEW.credit_note_id AND company_id=NEW.company_id;
    IF already_reversed + NEW.amount > original_amount THEN
        RAISE EXCEPTION 'customer credit note reversals exceed original credit' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION enforce_supplier_credit_note_reversal_limit()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE original_amount NUMERIC(19,4); already_reversed NUMERIC(19,4); source_invoice BIGINT;
BEGIN
    PERFORM 1 FROM suppliers WHERE id=NEW.supplier_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT invoice_id INTO source_invoice FROM supplier_credit_notes
      WHERE id=NEW.credit_note_id AND company_id=NEW.company_id;
    PERFORM 1 FROM supplier_invoices WHERE id=source_invoice AND company_id=NEW.company_id FOR UPDATE;
    SELECT amount INTO original_amount FROM supplier_credit_notes
      WHERE id=NEW.credit_note_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT COALESCE(SUM(amount),0) INTO already_reversed FROM supplier_credit_note_reversals
      WHERE credit_note_id=NEW.credit_note_id AND company_id=NEW.company_id;
    IF already_reversed + NEW.amount > original_amount THEN
        RAISE EXCEPTION 'supplier credit note reversals exceed original credit' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE FUNCTION prevent_financial_event_mutation()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'posted allocations and reversal events are immutable' USING ERRCODE='23514';
END;
$$;

CREATE TRIGGER trg_customer_allocation_reversal_limit BEFORE INSERT ON customer_allocation_reversals
    FOR EACH ROW EXECUTE FUNCTION enforce_customer_allocation_reversal_limit();
CREATE TRIGGER trg_supplier_allocation_reversal_limit BEFORE INSERT ON supplier_allocation_reversals
    FOR EACH ROW EXECUTE FUNCTION enforce_supplier_allocation_reversal_limit();
CREATE TRIGGER trg_customer_payment_reversal_limit BEFORE INSERT ON customer_payment_reversals
    FOR EACH ROW EXECUTE FUNCTION enforce_customer_payment_reversal_limit();
CREATE TRIGGER trg_supplier_payment_reversal_limit BEFORE INSERT ON supplier_payment_reversals
    FOR EACH ROW EXECUTE FUNCTION enforce_supplier_payment_reversal_limit();
CREATE TRIGGER trg_customer_credit_reversal_limit BEFORE INSERT ON customer_credit_note_reversals
    FOR EACH ROW EXECUTE FUNCTION enforce_customer_credit_note_reversal_limit();
CREATE TRIGGER trg_supplier_credit_reversal_limit BEFORE INSERT ON supplier_credit_note_reversals
    FOR EACH ROW EXECUTE FUNCTION enforce_supplier_credit_note_reversal_limit();
CREATE TRIGGER trg_customer_payment_reversal_open_session BEFORE INSERT ON customer_payment_reversals
    FOR EACH ROW EXECUTE FUNCTION enforce_account_payment_open_cash_session();
CREATE TRIGGER trg_supplier_payment_reversal_open_session BEFORE INSERT ON supplier_payment_reversals
    FOR EACH ROW EXECUTE FUNCTION enforce_account_payment_open_cash_session();

CREATE TRIGGER trg_customer_allocation_immutable BEFORE UPDATE OR DELETE ON customer_payment_allocations
    FOR EACH ROW EXECUTE FUNCTION prevent_financial_event_mutation();
CREATE TRIGGER trg_supplier_allocation_immutable BEFORE UPDATE OR DELETE ON supplier_payment_allocations
    FOR EACH ROW EXECUTE FUNCTION prevent_financial_event_mutation();
CREATE TRIGGER trg_customer_allocation_reversal_immutable BEFORE UPDATE OR DELETE ON customer_allocation_reversals
    FOR EACH ROW EXECUTE FUNCTION prevent_financial_event_mutation();
CREATE TRIGGER trg_supplier_allocation_reversal_immutable BEFORE UPDATE OR DELETE ON supplier_allocation_reversals
    FOR EACH ROW EXECUTE FUNCTION prevent_financial_event_mutation();
CREATE TRIGGER trg_customer_payment_reversal_immutable BEFORE UPDATE OR DELETE ON customer_payment_reversals
    FOR EACH ROW EXECUTE FUNCTION prevent_financial_event_mutation();
CREATE TRIGGER trg_supplier_payment_reversal_immutable BEFORE UPDATE OR DELETE ON supplier_payment_reversals
    FOR EACH ROW EXECUTE FUNCTION prevent_financial_event_mutation();
CREATE TRIGGER trg_customer_credit_reversal_immutable BEFORE UPDATE OR DELETE ON customer_credit_note_reversals
    FOR EACH ROW EXECUTE FUNCTION prevent_financial_event_mutation();
CREATE TRIGGER trg_supplier_credit_reversal_immutable BEFORE UPDATE OR DELETE ON supplier_credit_note_reversals
    FOR EACH ROW EXECUTE FUNCTION prevent_financial_event_mutation();

CREATE FUNCTION lock_customer_account_for_payment()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM 1 FROM customers
      WHERE id=NEW.customer_id AND company_id=NEW.company_id FOR UPDATE;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_customer_payment_lock_account
    BEFORE INSERT ON customer_payments
    FOR EACH ROW EXECUTE FUNCTION lock_customer_account_for_payment();
CREATE TRIGGER trg_customer_payment_open_cash_session
    BEFORE INSERT ON customer_payments
    FOR EACH ROW EXECUTE FUNCTION enforce_account_payment_open_cash_session();
CREATE TRIGGER trg_supplier_payment_open_cash_session
    BEFORE INSERT ON supplier_payments
    FOR EACH ROW EXECUTE FUNCTION enforce_account_payment_open_cash_session();

-- Allocation totals span rows. These triggers lock payment and target rows before
-- checking sums, so concurrent allocators cannot both spend the same remainder.
CREATE FUNCTION enforce_customer_payment_allocation_limits()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    payment_amount NUMERIC(19,4);
    sale_total NUMERIC(19,4);
    sale_status VARCHAR(20);
    allocated_payment NUMERIC(19,4);
    allocated_sale NUMERIC(19,4);
    direct_paid NUMERIC(19,4);
    credited NUMERIC(19,4);
BEGIN
    IF TG_OP = 'UPDATE' AND
       (NEW.company_id, NEW.payment_id, NEW.sale_id, NEW.customer_id, NEW.currency_code)
       IS DISTINCT FROM
       (OLD.company_id, OLD.payment_id, OLD.sale_id, OLD.customer_id, OLD.currency_code) THEN
        RAISE EXCEPTION 'allocation identity is immutable; create a correcting financial event'
            USING ERRCODE = '23514';
    END IF;
    SELECT amount INTO payment_amount FROM customer_payments
      WHERE id=NEW.payment_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT total_amount,status INTO sale_total,sale_status FROM sales
      WHERE id=NEW.sale_id AND company_id=NEW.company_id FOR UPDATE;
    IF sale_status IS DISTINCT FROM 'completed' THEN
        RAISE EXCEPTION 'customer allocation target must be a completed sale' USING ERRCODE='23514';
    END IF;
    SELECT COALESCE((SELECT SUM(amount) FROM customer_payment_allocations
                     WHERE payment_id=NEW.payment_id AND company_id=NEW.company_id),0)
         - COALESCE((SELECT SUM(amount) FROM customer_allocation_reversals
                     WHERE payment_id=NEW.payment_id AND company_id=NEW.company_id),0)
      INTO allocated_payment;
    IF allocated_payment + NEW.amount > payment_amount THEN
        RAISE EXCEPTION 'customer payment allocation exceeds payment amount' USING ERRCODE='23514';
    END IF;
    SELECT COALESCE((SELECT SUM(amount) FROM customer_payment_allocations
                     WHERE sale_id=NEW.sale_id AND company_id=NEW.company_id),0)
         - COALESCE((SELECT SUM(amount) FROM customer_allocation_reversals
                     WHERE sale_id=NEW.sale_id AND company_id=NEW.company_id),0)
      INTO allocated_sale;
    SELECT COALESCE(SUM(amount),0) INTO direct_paid FROM sale_payments
      WHERE sale_id=NEW.sale_id AND company_id=NEW.company_id;
    SELECT COALESCE((SELECT SUM(amount) FROM customer_credit_notes
                     WHERE sale_id=NEW.sale_id AND company_id=NEW.company_id),0)
         - COALESCE((SELECT SUM(r.amount) FROM customer_credit_note_reversals r
                     JOIN customer_credit_notes n ON n.id=r.credit_note_id AND n.company_id=r.company_id
                     WHERE n.sale_id=NEW.sale_id AND r.company_id=NEW.company_id),0)
      INTO credited;
    IF allocated_sale + NEW.amount + direct_paid + credited > sale_total THEN
        RAISE EXCEPTION 'customer sale allocation exceeds remaining sale balance' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_customer_payment_allocation_limits
    BEFORE INSERT ON customer_payment_allocations
    FOR EACH ROW EXECUTE FUNCTION enforce_customer_payment_allocation_limits();

CREATE FUNCTION enforce_supplier_payment_allocation_limits()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    payment_amount NUMERIC(19,4);
    invoice_total NUMERIC(19,4);
    invoice_status VARCHAR(20);
    allocated_payment NUMERIC(19,4);
    allocated_invoice NUMERIC(19,4);
    credited NUMERIC(19,4);
BEGIN
    IF TG_OP = 'UPDATE' AND
       (NEW.company_id, NEW.payment_id, NEW.invoice_id, NEW.supplier_id, NEW.currency_code)
       IS DISTINCT FROM
       (OLD.company_id, OLD.payment_id, OLD.invoice_id, OLD.supplier_id, OLD.currency_code) THEN
        RAISE EXCEPTION 'allocation identity is immutable; create a correcting financial event'
            USING ERRCODE = '23514';
    END IF;
    SELECT amount INTO payment_amount FROM supplier_payments
      WHERE id=NEW.payment_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT total_amount,status INTO invoice_total,invoice_status FROM supplier_invoices
      WHERE id=NEW.invoice_id AND company_id=NEW.company_id FOR UPDATE;
    IF invoice_status IS DISTINCT FROM 'posted' THEN
        RAISE EXCEPTION 'supplier allocation target must be a posted invoice' USING ERRCODE='23514';
    END IF;
    SELECT COALESCE((SELECT SUM(amount) FROM supplier_payment_allocations
                     WHERE payment_id=NEW.payment_id AND company_id=NEW.company_id),0)
         - COALESCE((SELECT SUM(amount) FROM supplier_allocation_reversals
                     WHERE payment_id=NEW.payment_id AND company_id=NEW.company_id),0)
      INTO allocated_payment;
    IF allocated_payment + NEW.amount > payment_amount THEN
        RAISE EXCEPTION 'supplier payment allocation exceeds payment amount' USING ERRCODE='23514';
    END IF;
    SELECT COALESCE((SELECT SUM(amount) FROM supplier_payment_allocations
                     WHERE invoice_id=NEW.invoice_id AND company_id=NEW.company_id),0)
         - COALESCE((SELECT SUM(amount) FROM supplier_allocation_reversals
                     WHERE invoice_id=NEW.invoice_id AND company_id=NEW.company_id),0)
      INTO allocated_invoice;
    SELECT COALESCE((SELECT SUM(amount) FROM supplier_credit_notes
                     WHERE invoice_id=NEW.invoice_id AND company_id=NEW.company_id),0)
         - COALESCE((SELECT SUM(r.amount) FROM supplier_credit_note_reversals r
                     JOIN supplier_credit_notes n ON n.id=r.credit_note_id AND n.company_id=r.company_id
                     WHERE n.invoice_id=NEW.invoice_id AND r.company_id=NEW.company_id),0)
      INTO credited;
    IF allocated_invoice + NEW.amount + credited > invoice_total THEN
        RAISE EXCEPTION 'supplier invoice allocation exceeds remaining invoice balance' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_supplier_payment_allocation_limits
    BEFORE INSERT ON supplier_payment_allocations
    FOR EACH ROW EXECUTE FUNCTION enforce_supplier_payment_allocation_limits();

CREATE FUNCTION enforce_customer_credit_note_limit()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    sale_total NUMERIC(19,4);
    sale_status VARCHAR(20);
    spent NUMERIC(19,4);
    paid NUMERIC(19,4);
    allocated NUMERIC(19,4);
    credited NUMERIC(19,4);
    current_balance NUMERIC(19,4);
BEGIN
    PERFORM 1 FROM customers WHERE id=NEW.customer_id AND company_id=NEW.company_id FOR UPDATE;
    IF NEW.sale_id IS NOT NULL THEN
        SELECT total_amount,status INTO sale_total,sale_status FROM sales
          WHERE id=NEW.sale_id AND company_id=NEW.company_id FOR UPDATE;
        IF sale_status IS DISTINCT FROM 'completed' THEN
            RAISE EXCEPTION 'customer credit note target must be a completed sale' USING ERRCODE='23514';
        END IF;
        SELECT COALESCE((SELECT SUM(amount) FROM customer_payment_allocations
                         WHERE sale_id=NEW.sale_id AND company_id=NEW.company_id),0)
             - COALESCE((SELECT SUM(amount) FROM customer_allocation_reversals
                         WHERE sale_id=NEW.sale_id AND company_id=NEW.company_id),0)
          INTO allocated;
        SELECT COALESCE(SUM(amount),0) INTO paid FROM sale_payments
          WHERE sale_id=NEW.sale_id AND company_id=NEW.company_id;
        SELECT COALESCE((SELECT SUM(amount) FROM customer_credit_notes
                         WHERE sale_id=NEW.sale_id AND company_id=NEW.company_id),0)
             - COALESCE((SELECT SUM(r.amount) FROM customer_credit_note_reversals r
                         JOIN customer_credit_notes n ON n.id=r.credit_note_id AND n.company_id=r.company_id
                         WHERE n.sale_id=NEW.sale_id AND r.company_id=NEW.company_id),0)
          INTO credited;
        spent := allocated + paid + credited + NEW.amount;
        IF spent > sale_total THEN
            RAISE EXCEPTION 'customer credit note exceeds remaining sale balance' USING ERRCODE='23514';
        END IF;
    ELSE
        -- Serialize account-level credits with checkout and other customer events.
        SELECT balance INTO current_balance FROM customer_account_balances
          WHERE customer_id=NEW.customer_id AND company_id=NEW.company_id;
        IF COALESCE(current_balance,0) < NEW.amount THEN
            RAISE EXCEPTION 'customer credit note exceeds current account balance' USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_customer_credit_note_limit
    BEFORE INSERT ON customer_credit_notes
    FOR EACH ROW EXECUTE FUNCTION enforce_customer_credit_note_limit();

CREATE FUNCTION enforce_supplier_credit_note_limit()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    invoice_total NUMERIC(19,4);
    invoice_status VARCHAR(20);
    allocated NUMERIC(19,4);
    credited NUMERIC(19,4);
BEGIN
    PERFORM 1 FROM suppliers WHERE id=NEW.supplier_id AND company_id=NEW.company_id FOR UPDATE;
    SELECT total_amount,status INTO invoice_total,invoice_status FROM supplier_invoices
      WHERE id=NEW.invoice_id AND company_id=NEW.company_id FOR UPDATE;
    IF invoice_status IS DISTINCT FROM 'posted' THEN
        RAISE EXCEPTION 'supplier credit note target must be a posted invoice' USING ERRCODE='23514';
    END IF;
    SELECT COALESCE((SELECT SUM(amount) FROM supplier_payment_allocations
                     WHERE invoice_id=NEW.invoice_id AND company_id=NEW.company_id),0)
         - COALESCE((SELECT SUM(amount) FROM supplier_allocation_reversals
                     WHERE invoice_id=NEW.invoice_id AND company_id=NEW.company_id),0)
      INTO allocated;
    SELECT COALESCE((SELECT SUM(amount) FROM supplier_credit_notes
                     WHERE invoice_id=NEW.invoice_id AND company_id=NEW.company_id),0)
         - COALESCE((SELECT SUM(r.amount) FROM supplier_credit_note_reversals r
                     JOIN supplier_credit_notes n ON n.id=r.credit_note_id AND n.company_id=r.company_id
                     WHERE n.invoice_id=NEW.invoice_id AND r.company_id=NEW.company_id),0)
      INTO credited;
    IF allocated + credited + NEW.amount > invoice_total THEN
        RAISE EXCEPTION 'supplier credit note exceeds remaining invoice balance' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_supplier_credit_note_limit
    BEFORE INSERT ON supplier_credit_notes
    FOR EACH ROW EXECUTE FUNCTION enforce_supplier_credit_note_limit();

CREATE FUNCTION enforce_supplier_invoice_item_source_consistency()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    invoice_order BIGINT;
    item_order BIGINT;
    receipt_order BIGINT;
    receipt_order_item BIGINT;
BEGIN
    SELECT purchase_order_id INTO invoice_order FROM supplier_invoices
      WHERE id=NEW.invoice_id AND company_id=NEW.company_id FOR UPDATE;
    IF NEW.purchase_order_item_id IS NOT NULL THEN
        SELECT purchase_order_id INTO item_order FROM purchase_order_items
          WHERE id=NEW.purchase_order_item_id AND company_id=NEW.company_id;
        IF invoice_order IS DISTINCT FROM item_order THEN
            RAISE EXCEPTION 'invoice line purchase order item does not match invoice purchase order'
                USING ERRCODE='23514';
        END IF;
    END IF;
    IF NEW.goods_receipt_item_id IS NOT NULL THEN
        SELECT poi.purchase_order_id,gri.purchase_order_item_id
          INTO receipt_order,receipt_order_item
          FROM goods_receipt_items gri
          JOIN purchase_order_items poi
            ON poi.id=gri.purchase_order_item_id AND poi.company_id=gri.company_id
          WHERE gri.id=NEW.goods_receipt_item_id AND gri.company_id=NEW.company_id;
        IF invoice_order IS DISTINCT FROM receipt_order
           OR (NEW.purchase_order_item_id IS NOT NULL
               AND NEW.purchase_order_item_id IS DISTINCT FROM receipt_order_item) THEN
            RAISE EXCEPTION 'invoice line receipt item does not match invoice purchase order item'
                USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_supplier_invoice_item_source_consistency
    BEFORE INSERT OR UPDATE ON supplier_invoice_items
    FOR EACH ROW EXECUTE FUNCTION enforce_supplier_invoice_item_source_consistency();

-- A posted invoice is immutable. Drafts remain editable; corrections after
-- posting use supplier credit notes. Posting reconciles persisted line amounts.
CREATE FUNCTION enforce_supplier_invoice_posting()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    line_count BIGINT;
    line_total NUMERIC(19,4);
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.status = 'posted' THEN
            RAISE EXCEPTION 'posted supplier invoices are immutable; use a credit note'
                USING ERRCODE='23514';
        END IF;
        RETURN OLD;
    END IF;
    IF OLD.status = 'posted' THEN
        RAISE EXCEPTION 'posted supplier invoices are immutable; use a credit note'
            USING ERRCODE='23514';
    END IF;
    IF NEW.status = 'posted' THEN
        SELECT COUNT(*), COALESCE(SUM(line_total),0)
          INTO line_count,line_total FROM supplier_invoice_items
          WHERE invoice_id=NEW.id AND company_id=NEW.company_id;
        IF line_count = 0 OR line_total <> NEW.subtotal + NEW.tax_total - NEW.discount_total THEN
            RAISE EXCEPTION 'supplier invoice lines do not reconcile to header totals'
                USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_supplier_invoice_posting
    BEFORE UPDATE OR DELETE ON supplier_invoices
    FOR EACH ROW EXECUTE FUNCTION enforce_supplier_invoice_posting();

CREATE FUNCTION prevent_posted_supplier_invoice_item_change()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    parent_status VARCHAR(20);
BEGIN
    IF TG_OP='DELETE' THEN
        SELECT status INTO parent_status FROM supplier_invoices
          WHERE id=OLD.invoice_id AND company_id=OLD.company_id FOR UPDATE;
    ELSE
        SELECT status INTO parent_status FROM supplier_invoices
          WHERE id=NEW.invoice_id AND company_id=NEW.company_id FOR UPDATE;
    END IF;
    IF parent_status = 'posted' THEN
        RAISE EXCEPTION 'posted supplier invoice lines are immutable; use a credit note'
            USING ERRCODE='23514';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_supplier_invoice_items_immutable_when_posted
    BEFORE INSERT OR UPDATE OR DELETE ON supplier_invoice_items
    FOR EACH ROW EXECUTE FUNCTION prevent_posted_supplier_invoice_item_change();

CREATE FUNCTION prevent_completed_sale_mutation()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.status='completed' THEN
        RAISE EXCEPTION 'completed sales are immutable; use returns, refunds, voids, or credit notes'
            USING ERRCODE='23514';
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_completed_sales_immutable
    BEFORE UPDATE OR DELETE ON sales
    FOR EACH ROW EXECUTE FUNCTION prevent_completed_sale_mutation();

CREATE FUNCTION prevent_completed_sale_child_mutation()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    sale_status VARCHAR(20);
    parent_sale_id BIGINT;
    parent_company_id UUID;
BEGIN
    IF TG_OP='UPDATE' AND
       (NEW.sale_id,NEW.company_id) IS DISTINCT FROM (OLD.sale_id,OLD.company_id) THEN
        RAISE EXCEPTION 'completed sale line or payment identity cannot be changed'
            USING ERRCODE='23514';
    END IF;
    IF TG_OP='DELETE' THEN
        parent_sale_id := OLD.sale_id;
        parent_company_id := OLD.company_id;
    ELSE
        parent_sale_id := NEW.sale_id;
        parent_company_id := NEW.company_id;
    END IF;
    SELECT status INTO sale_status FROM sales
      WHERE id=parent_sale_id AND company_id=parent_company_id FOR UPDATE;
    IF sale_status='completed' THEN
        RAISE EXCEPTION 'completed sale lines and payments are immutable'
            USING ERRCODE='23514';
    END IF;
    IF TG_OP='DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER trg_completed_sale_items_immutable
    BEFORE INSERT OR UPDATE OR DELETE ON sale_items
    FOR EACH ROW EXECUTE FUNCTION prevent_completed_sale_child_mutation();
CREATE TRIGGER trg_completed_sale_payments_immutable
    BEFORE INSERT OR UPDATE OR DELETE ON sale_payments
    FOR EACH ROW EXECUTE FUNCTION prevent_completed_sale_child_mutation();

-- Payment and credit-note rows represent posted events. Corrections are additive
-- financial events (refund/credit/reversal), never edits or deletes.
CREATE FUNCTION prevent_posted_account_event_mutation()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'posted account events are immutable; record a reversal or credit note'
        USING ERRCODE='23514';
    RETURN NULL;
END;
$$;
CREATE TRIGGER trg_customer_payments_immutable
    BEFORE UPDATE OR DELETE ON customer_payments
    FOR EACH ROW EXECUTE FUNCTION prevent_posted_account_event_mutation();
CREATE TRIGGER trg_supplier_payments_immutable
    BEFORE UPDATE OR DELETE ON supplier_payments
    FOR EACH ROW EXECUTE FUNCTION prevent_posted_account_event_mutation();
CREATE TRIGGER trg_customer_credit_notes_immutable
    BEFORE UPDATE OR DELETE ON customer_credit_notes
    FOR EACH ROW EXECUTE FUNCTION prevent_posted_account_event_mutation();
CREATE TRIGGER trg_supplier_credit_notes_immutable
    BEFORE UPDATE OR DELETE ON supplier_credit_notes
    FOR EACH ROW EXECUTE FUNCTION prevent_posted_account_event_mutation();
CREATE TRIGGER trg_sale_refunds_immutable
    BEFORE UPDATE OR DELETE ON sale_refunds
    FOR EACH ROW EXECUTE FUNCTION prevent_financial_event_mutation();

-- Replace the current supplier summary with currency-separated derived balances.
CREATE OR REPLACE VIEW customer_account_balances AS
SELECT c.company_id,c.id AS customer_id,
 COALESCE((SELECT SUM(s.total_amount) FROM sales s WHERE s.company_id=c.company_id AND s.customer_id=c.id AND s.status='completed'),0)
 - COALESCE((SELECT SUM(p.amount) FROM sale_payments p JOIN sales s ON s.id=p.sale_id AND s.company_id=p.company_id AND s.currency_code=p.currency_code WHERE s.company_id=c.company_id AND s.customer_id=c.id AND s.status='completed'),0)
 - COALESCE((SELECT SUM(p.amount) FROM customer_payments p WHERE p.company_id=c.company_id AND p.customer_id=c.id),0)
 - COALESCE((SELECT SUM(n.amount) FROM customer_credit_notes n WHERE n.company_id=c.company_id AND n.customer_id=c.id),0)
 + COALESCE((SELECT SUM(r.amount) FROM customer_payment_reversals r WHERE r.company_id=c.company_id AND r.customer_id=c.id),0)
 + COALESCE((SELECT SUM(r.amount) FROM customer_credit_note_reversals r WHERE r.company_id=c.company_id AND r.customer_id=c.id),0) AS balance,
 co.default_currency AS currency_code
FROM customers c JOIN companies co ON co.id=c.company_id;

CREATE OR REPLACE VIEW supplier_account_balances AS
WITH supplier_currency AS (
    SELECT s.company_id,s.id AS supplier_id,c.default_currency AS currency_code
    FROM suppliers s JOIN companies c ON c.id=s.company_id
    UNION
    SELECT company_id,supplier_id,currency_code FROM supplier_invoices
    UNION
    SELECT company_id,supplier_id,currency_code FROM supplier_payments
    UNION
    SELECT company_id,supplier_id,currency_code FROM supplier_payment_reversals
    UNION
    SELECT company_id,supplier_id,currency_code FROM supplier_credit_notes
    UNION
    SELECT company_id,supplier_id,currency_code FROM supplier_credit_note_reversals
)
SELECT sc.company_id,sc.supplier_id,
 COALESCE((SELECT SUM(i.total_amount) FROM supplier_invoices i WHERE i.company_id=sc.company_id AND i.supplier_id=sc.supplier_id AND i.currency_code=sc.currency_code AND i.status='posted'),0)
 - COALESCE((SELECT SUM(p.amount) FROM supplier_payments p WHERE p.company_id=sc.company_id AND p.supplier_id=sc.supplier_id AND p.currency_code=sc.currency_code),0)
 - COALESCE((SELECT SUM(n.amount) FROM supplier_credit_notes n WHERE n.company_id=sc.company_id AND n.supplier_id=sc.supplier_id AND n.currency_code=sc.currency_code),0)
 + COALESCE((SELECT SUM(r.amount) FROM supplier_payment_reversals r WHERE r.company_id=sc.company_id AND r.supplier_id=sc.supplier_id AND r.currency_code=sc.currency_code),0)
 + COALESCE((SELECT SUM(r.amount) FROM supplier_credit_note_reversals r WHERE r.company_id=sc.company_id AND r.supplier_id=sc.supplier_id AND r.currency_code=sc.currency_code),0) AS balance,
 sc.currency_code
FROM supplier_currency sc;

-- Apply financial permissions only to active system roles. Existing custom-role
-- grants are intentionally not revoked because the schema has no grant provenance.
INSERT INTO permissions(permission_key,resource,action,description)
VALUES ('customers.profile.manage','customers','profile_manage','Create and edit customer profile details')
ON CONFLICT(permission_key) DO NOTHING;

-- Preserve the old customers.manage capability as profile management for every
-- role that already had it. The HTTP model no longer accepts credit-policy fields.
INSERT INTO role_permissions(role_id,permission_id)
SELECT old_grant.role_id,new_permission.id
FROM role_permissions old_grant
JOIN permissions old_permission ON old_permission.id=old_grant.permission_id
JOIN permissions new_permission ON new_permission.permission_key='customers.profile.manage'
WHERE old_permission.permission_key='customers.manage'
ON CONFLICT(role_id,permission_id) DO NOTHING;

INSERT INTO role_permissions(role_id,permission_id)
SELECT r.id,p.id FROM roles r CROSS JOIN permissions p
WHERE r.is_system_role=TRUE AND r.is_active=TRUE
  AND r.role_key IN ('owner','admin','manager')
  AND p.permission_key IN ('customers.view','customers.profile.manage','customers.manage','customers.credit','customers.payment','suppliers.view','suppliers.manage','suppliers.invoice','suppliers.payment')
ON CONFLICT(role_id,permission_id) DO NOTHING;
INSERT INTO role_permissions(role_id,permission_id)
SELECT r.id,p.id FROM roles r CROSS JOIN permissions p
WHERE r.is_system_role=TRUE AND r.is_active=TRUE AND r.role_key='cashier'
  AND p.permission_key IN ('customers.view','customers.payment','suppliers.view')
ON CONFLICT(role_id,permission_id) DO NOTHING;
