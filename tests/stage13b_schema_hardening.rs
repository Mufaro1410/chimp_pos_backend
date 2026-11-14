const HARDENING: &str =
    include_str!("../migrations/20261112120000_stage_13b_financial_integrity.sql");

#[test]
fn tenant_and_party_relationships_use_company_aware_keys() {
    for fragment in [
        "fk_supplier_invoice_items_purchase_item_company",
        "fk_supplier_invoice_items_receipt_item_company",
        "fk_supplier_invoices_order_supplier",
        "fk_customer_allocations_payment_party_currency",
        "fk_customer_allocations_sale_party_currency",
        "fk_supplier_allocations_payment_party_currency",
        "fk_supplier_allocations_invoice_party_currency",
        "fk_customer_credit_notes_sale_party_currency",
        "fk_supplier_credit_notes_invoice_party_currency",
        "uq_customer_allocation_reversal_source",
        "uq_supplier_allocation_reversal_source",
    ] {
        assert!(
            HARDENING.contains(fragment),
            "missing migration clause: {fragment}"
        );
    }
}

#[test]
fn allocation_and_credit_limits_lock_their_financial_parents() {
    for fragment in [
        "FUNCTION enforce_customer_payment_allocation_limits()",
        "FUNCTION enforce_supplier_payment_allocation_limits()",
        "customer payment allocation exceeds payment amount",
        "customer sale allocation exceeds remaining sale balance",
        "supplier payment allocation exceeds payment amount",
        "supplier invoice allocation exceeds remaining invoice balance",
        "FUNCTION enforce_customer_credit_note_limit()",
        "FUNCTION enforce_supplier_credit_note_limit()",
        "FOR UPDATE",
    ] {
        assert!(
            HARDENING.contains(fragment),
            "missing invariant: {fragment}"
        );
    }
}

#[test]
fn currency_is_persisted_and_balances_remain_currency_scoped() {
    for fragment in [
        "ADD COLUMN currency_code CHAR(3)",
        "fk_sales_company_currency",
        "fk_customer_payments_company_currency",
        "chk_supplier_invoices_currency_format",
        "supplier_account_balances",
        "sc.currency_code",
        "cash account payment currency must match company cash currency",
    ] {
        assert!(
            HARDENING.contains(fragment),
            "missing currency rule: {fragment}"
        );
    }
}

#[test]
fn posted_financial_rows_are_immutable_and_invoice_posting_reconciles() {
    for fragment in [
        "trg_supplier_invoice_posting",
        "trg_supplier_invoice_items_immutable_when_posted",
        "trg_completed_sales_immutable",
        "trg_completed_sale_payments_immutable",
        "trg_customer_payments_immutable",
        "trg_supplier_payments_immutable",
        "trg_customer_credit_notes_immutable",
        "trg_supplier_credit_notes_immutable",
        "trg_customer_allocation_immutable",
        "trg_supplier_allocation_immutable",
        "trg_sale_refunds_immutable",
        "supplier invoice lines do not reconcile to header totals",
    ] {
        assert!(
            HARDENING.contains(fragment),
            "missing posting rule: {fragment}"
        );
    }
}

#[test]
fn reversal_events_are_append_only_capped_and_currency_scoped() {
    for fragment in [
        "CREATE TABLE customer_allocation_reversals",
        "CREATE TABLE supplier_allocation_reversals",
        "CREATE TABLE customer_payment_reversals",
        "CREATE TABLE supplier_payment_reversals",
        "CREATE TABLE customer_credit_note_reversals",
        "CREATE TABLE supplier_credit_note_reversals",
        "enforce_customer_payment_reversal_limit",
        "enforce_supplier_payment_reversal_limit",
        "enforce_customer_credit_note_reversal_limit",
        "enforce_supplier_credit_note_reversal_limit",
        "payment reversal exceeds unallocated and reversed allocation amount",
        "customer_payment_reversals",
        "supplier_payment_reversals",
    ] {
        assert!(
            HARDENING.contains(fragment),
            "missing reversal rule: {fragment}"
        );
    }
}

#[test]
fn historical_currency_and_cash_attribution_are_approval_gated() {
    let approvals =
        include_str!("../migrations/20261112110000_stage_13b_reconciliation_approvals.sql");
    for fragment in [
        "stage13b_sale_currency_approvals",
        "stage13b_customer_payment_currency_approvals",
        "stage13b_customer_credit_note_currency_approvals",
        "stage13b_supplier_payment_currency_approvals",
        "stage13b_cash_event_reconciliations",
        "approved_by",
        "evidence_reference",
        "outside_session_exception",
        "financial.reconciliation.approve",
        "active owner or admin",
    ] {
        assert!(
            approvals.contains(fragment),
            "missing approval field: {fragment}"
        );
    }
    assert!(HARDENING.contains("historical sale currency approval is unresolved"));
    assert!(HARDENING.contains("cash refund session attribution is unresolved"));
    assert!(HARDENING.contains("supplier payment currency approval is unresolved"));
}

#[test]
fn customer_credit_policy_has_a_separate_permission_boundary() {
    let models = include_str!("../src/models/trading_parties.rs");
    let middleware = include_str!("../src/utils/permission_middleware.rs");
    let service = include_str!("../src/services/trading_parties_service.rs");
    assert!(models.contains("CustomerCreditPolicyUpdate"));
    assert!(middleware.contains("customers.profile.manage"));
    assert!(middleware.contains("Some(\"customers.credit\")"));
    assert!(service.contains("p.permission_key='customers.credit'"));
}

#[test]
fn grants_are_limited_to_active_system_roles() {
    assert!(HARDENING.contains("r.is_system_role=TRUE AND r.is_active=TRUE"));
    assert!(HARDENING.contains("role_key IN ('owner','admin','manager')"));
    assert!(HARDENING.contains("role_key='cashier'"));
}
