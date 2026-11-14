-- Stage 13B reconciliation gate.
-- This migration creates durable approval records and seeds unresolved rows.
-- It intentionally does not infer currencies or session attribution.

INSERT INTO permissions(permission_key,resource,action,description)
VALUES ('financial.reconciliation.approve','financial','reconciliation_approve','Approve historical financial currency and cash reconciliation')
ON CONFLICT(permission_key) DO NOTHING;

INSERT INTO role_permissions(role_id,permission_id)
SELECT r.id,p.id FROM roles r JOIN permissions p ON p.permission_key='financial.reconciliation.approve'
WHERE r.is_system_role=TRUE AND r.is_active=TRUE AND r.role_key IN ('owner','admin')
ON CONFLICT(role_id,permission_id) DO NOTHING;

ALTER TABLE customer_credit_notes
    ADD CONSTRAINT uq_customer_credit_notes_id_company UNIQUE (id, company_id);

CREATE TABLE stage13b_sale_currency_approvals (
    company_id UUID NOT NULL,
    sale_id BIGINT NOT NULL,
    approved_currency CHAR(3),
    evidence_reference TEXT,
    approval_notes TEXT,
    approved_by UUID,
    approved_at TIMESTAMPTZ,
    PRIMARY KEY (company_id, sale_id),
    FOREIGN KEY (sale_id, company_id) REFERENCES sales(id, company_id) ON DELETE RESTRICT,
    FOREIGN KEY (approved_by, company_id) REFERENCES users(id, company_id) ON DELETE RESTRICT,
    CHECK (approved_currency IS NULL OR approved_currency ~ '^[A-Z]{3}$'),
    CHECK (
        (approved_currency IS NULL AND approved_by IS NULL AND approved_at IS NULL)
        OR
        (approved_currency IS NOT NULL AND approved_by IS NOT NULL AND approved_at IS NOT NULL
            AND length(trim(COALESCE(evidence_reference, ''))) > 0)
    )
);

INSERT INTO stage13b_sale_currency_approvals(company_id, sale_id)
SELECT company_id, id FROM sales
ON CONFLICT (company_id, sale_id) DO NOTHING;

CREATE TABLE stage13b_customer_payment_currency_approvals (
    company_id UUID NOT NULL,
    payment_id BIGINT NOT NULL,
    customer_id BIGINT NOT NULL,
    approved_currency CHAR(3),
    evidence_reference TEXT,
    approval_notes TEXT,
    approved_by UUID,
    approved_at TIMESTAMPTZ,
    PRIMARY KEY (company_id, payment_id),
    FOREIGN KEY (payment_id, company_id) REFERENCES customer_payments(id, company_id) ON DELETE RESTRICT,
    FOREIGN KEY (customer_id, company_id) REFERENCES customers(id, company_id) ON DELETE RESTRICT,
    FOREIGN KEY (approved_by, company_id) REFERENCES users(id, company_id) ON DELETE RESTRICT,
    CHECK (approved_currency IS NULL OR approved_currency ~ '^[A-Z]{3}$'),
    CHECK (
        (approved_currency IS NULL AND approved_by IS NULL AND approved_at IS NULL)
        OR (approved_currency IS NOT NULL AND approved_by IS NOT NULL AND approved_at IS NOT NULL
            AND length(trim(COALESCE(evidence_reference, ''))) > 0)
    )
);
INSERT INTO stage13b_customer_payment_currency_approvals(company_id,payment_id,customer_id)
SELECT company_id,id,customer_id FROM customer_payments
ON CONFLICT (company_id,payment_id) DO NOTHING;

CREATE TABLE stage13b_customer_credit_note_currency_approvals (
    company_id UUID NOT NULL,
    credit_note_id BIGINT NOT NULL,
    customer_id BIGINT NOT NULL,
    approved_currency CHAR(3),
    evidence_reference TEXT,
    approval_notes TEXT,
    approved_by UUID,
    approved_at TIMESTAMPTZ,
    PRIMARY KEY (company_id, credit_note_id),
    FOREIGN KEY (credit_note_id, company_id) REFERENCES customer_credit_notes(id, company_id) ON DELETE RESTRICT,
    FOREIGN KEY (customer_id, company_id) REFERENCES customers(id, company_id) ON DELETE RESTRICT,
    FOREIGN KEY (approved_by, company_id) REFERENCES users(id, company_id) ON DELETE RESTRICT,
    CHECK (approved_currency IS NULL OR approved_currency ~ '^[A-Z]{3}$'),
    CHECK (
        (approved_currency IS NULL AND approved_by IS NULL AND approved_at IS NULL)
        OR (approved_currency IS NOT NULL AND approved_by IS NOT NULL AND approved_at IS NOT NULL
            AND length(trim(COALESCE(evidence_reference, ''))) > 0)
    )
);
INSERT INTO stage13b_customer_credit_note_currency_approvals(company_id,credit_note_id,customer_id)
SELECT company_id,id,customer_id FROM customer_credit_notes
ON CONFLICT (company_id,credit_note_id) DO NOTHING;

CREATE TABLE stage13b_supplier_payment_currency_approvals (
    company_id UUID NOT NULL,
    payment_id BIGINT NOT NULL,
    supplier_id BIGINT NOT NULL,
    approved_currency CHAR(3),
    evidence_reference TEXT,
    approval_notes TEXT,
    approved_by UUID,
    approved_at TIMESTAMPTZ,
    PRIMARY KEY (company_id, payment_id),
    FOREIGN KEY (payment_id, company_id) REFERENCES supplier_payments(id, company_id) ON DELETE RESTRICT,
    FOREIGN KEY (supplier_id, company_id) REFERENCES suppliers(id, company_id) ON DELETE RESTRICT,
    FOREIGN KEY (approved_by, company_id) REFERENCES users(id, company_id) ON DELETE RESTRICT,
    CHECK (approved_currency IS NULL OR approved_currency ~ '^[A-Z]{3}$'),
    CHECK (
        (approved_currency IS NULL AND approved_by IS NULL AND approved_at IS NULL)
        OR
        (approved_currency IS NOT NULL AND approved_by IS NOT NULL AND approved_at IS NOT NULL
            AND length(trim(COALESCE(evidence_reference, ''))) > 0)
    )
);

INSERT INTO stage13b_supplier_payment_currency_approvals(company_id, payment_id, supplier_id)
SELECT company_id, id, supplier_id FROM supplier_payments
ON CONFLICT (company_id, payment_id) DO NOTHING;

CREATE TABLE stage13b_cash_event_reconciliations (
    company_id UUID NOT NULL,
    event_type VARCHAR(20) NOT NULL CHECK (event_type IN ('sale_payment', 'sale_refund')),
    event_id BIGINT NOT NULL,
    disposition VARCHAR(30) NOT NULL DEFAULT 'pending'
        CHECK (disposition IN ('pending', 'verified_session', 'outside_session_exception')),
    approved_session_id BIGINT,
    evidence_reference TEXT,
    approval_notes TEXT,
    approved_by UUID,
    approved_at TIMESTAMPTZ,
    PRIMARY KEY (company_id, event_type, event_id),
    FOREIGN KEY (approved_session_id, company_id)
        REFERENCES cash_register_sessions(id, company_id) ON DELETE RESTRICT,
    FOREIGN KEY (approved_by, company_id) REFERENCES users(id, company_id) ON DELETE RESTRICT,
    CHECK (
        (disposition = 'pending' AND approved_session_id IS NULL
            AND approved_by IS NULL AND approved_at IS NULL)
        OR (disposition = 'verified_session' AND approved_session_id IS NOT NULL
            AND approved_by IS NOT NULL AND approved_at IS NOT NULL
            AND length(trim(COALESCE(evidence_reference, ''))) > 0)
        OR (disposition = 'outside_session_exception' AND approved_session_id IS NULL
            AND approved_by IS NOT NULL AND approved_at IS NOT NULL
            AND length(trim(COALESCE(evidence_reference, ''))) > 0
            AND length(trim(COALESCE(approval_notes, ''))) > 0)
    )
);

INSERT INTO stage13b_cash_event_reconciliations(company_id, event_type, event_id)
SELECT company_id, 'sale_payment', id
FROM sale_payments
WHERE payment_method = 'cash' AND cash_session_id IS NULL
ON CONFLICT (company_id, event_type, event_id) DO NOTHING;

INSERT INTO stage13b_cash_event_reconciliations(company_id, event_type, event_id)
SELECT company_id, 'sale_refund', id
FROM sale_refunds
WHERE payment_method = 'cash' AND cash_session_id IS NULL
ON CONFLICT (company_id, event_type, event_id) DO NOTHING;

CREATE FUNCTION prevent_approved_stage13b_reconciliation_change()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP='DELETE' THEN
        RAISE EXCEPTION 'historical reconciliation rows are retained';
    END IF;
    IF OLD.approved_by IS NOT NULL THEN
        RAISE EXCEPTION 'approved historical reconciliation is immutable';
    END IF;
    IF NEW.approved_by IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM user_roles ur
        JOIN roles r ON r.id=ur.role_id AND r.company_id=ur.company_id
        JOIN role_permissions rp ON rp.role_id=r.id
        JOIN permissions p ON p.id=rp.permission_id
        WHERE ur.user_id=NEW.approved_by AND ur.company_id=NEW.company_id
          AND r.is_system_role=TRUE AND r.is_active=TRUE
          AND (ur.expires_at IS NULL OR ur.expires_at>NOW())
          AND p.permission_key='financial.reconciliation.approve'
    ) THEN
        RAISE EXCEPTION 'financial reconciliation approval requires an authorized active owner or admin';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER trg_stage13b_sale_currency_approval_immutable
BEFORE INSERT OR UPDATE OR DELETE ON stage13b_sale_currency_approvals
FOR EACH ROW EXECUTE FUNCTION prevent_approved_stage13b_reconciliation_change();
CREATE TRIGGER trg_stage13b_customer_payment_currency_approval_immutable
BEFORE INSERT OR UPDATE OR DELETE ON stage13b_customer_payment_currency_approvals
FOR EACH ROW EXECUTE FUNCTION prevent_approved_stage13b_reconciliation_change();
CREATE TRIGGER trg_stage13b_customer_credit_currency_approval_immutable
BEFORE INSERT OR UPDATE OR DELETE ON stage13b_customer_credit_note_currency_approvals
FOR EACH ROW EXECUTE FUNCTION prevent_approved_stage13b_reconciliation_change();
CREATE TRIGGER trg_stage13b_supplier_currency_approval_immutable
BEFORE INSERT OR UPDATE OR DELETE ON stage13b_supplier_payment_currency_approvals
FOR EACH ROW EXECUTE FUNCTION prevent_approved_stage13b_reconciliation_change();
CREATE TRIGGER trg_stage13b_cash_reconciliation_immutable
BEFORE INSERT OR UPDATE OR DELETE ON stage13b_cash_event_reconciliations
FOR EACH ROW EXECUTE FUNCTION prevent_approved_stage13b_reconciliation_change();
