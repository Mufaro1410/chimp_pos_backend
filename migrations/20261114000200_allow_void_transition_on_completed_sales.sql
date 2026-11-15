-- The Stage 13B immutability trigger blocked *every* update of a completed sale, including the
-- completed -> cancelled transition that the void workflow performs (so every void failed).
-- Completed sales stay immutable, except for that single transition, which is allowed only when
-- a void record exists for the sale and no monetary / identity field changes.
CREATE OR REPLACE FUNCTION prevent_completed_sale_mutation()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'UPDATE'
       AND OLD.status = 'completed'
       AND NEW.status = 'cancelled'
       AND EXISTS (
            SELECT 1 FROM sale_voids v
            WHERE v.sale_id = OLD.id AND v.company_id = OLD.company_id
       )
       AND NEW.id = OLD.id
       AND NEW.company_id = OLD.company_id
       AND NEW.sale_number = OLD.sale_number
       AND NEW.location_id = OLD.location_id
       AND NEW.customer_id IS NOT DISTINCT FROM OLD.customer_id
       AND NEW.currency_code = OLD.currency_code
       AND NEW.subtotal = OLD.subtotal
       AND NEW.discount_total = OLD.discount_total
       AND NEW.tax_total = OLD.tax_total
       AND NEW.total_amount = OLD.total_amount
    THEN
        RETURN NEW;
    END IF;

    IF OLD.status = 'completed' THEN
        RAISE EXCEPTION 'completed sales are immutable; use returns, refunds, voids, or credit notes'
            USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END;
$$;
