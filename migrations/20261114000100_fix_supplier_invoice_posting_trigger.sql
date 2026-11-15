-- The original trigger declared a PL/pgSQL variable named line_total, which collides with the
-- supplier_invoice_items.line_total column ("column reference line_total is ambiguous"),
-- making every supplier invoice fail to post. Same logic, unambiguous variable names.
CREATE OR REPLACE FUNCTION enforce_supplier_invoice_posting()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    v_line_count BIGINT;
    v_lines_total NUMERIC(19,4);
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
        SELECT COUNT(*), COALESCE(SUM(i.line_total),0)
          INTO v_line_count, v_lines_total
          FROM supplier_invoice_items i
          WHERE i.invoice_id = NEW.id AND i.company_id = NEW.company_id;
        IF v_line_count = 0
           OR v_lines_total <> NEW.subtotal + NEW.tax_total - NEW.discount_total THEN
            RAISE EXCEPTION 'supplier invoice lines do not reconcile to header totals'
                USING ERRCODE='23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
