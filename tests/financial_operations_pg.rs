//! Opt-in financial workflow tests. Requires an already-provisioned, disposable
//! schema; this test never runs migrations and refuses non-test database names.
use chimpPOS_backend::{
    models::financial_operations::{
        AllocationInput, CreditNoteCreate, CustomerPaymentCreate, SupplierInvoiceCreate,
        SupplierInvoiceLineInput, SupplierPaymentCreate,
    },
    services::financial_operations_service as finance,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::PgPool;
use std::{env, str::FromStr};
use uuid::Uuid;

#[derive(Deserialize)]
struct Fixtures {
    company_id: Uuid,
    foreign_company_id: Uuid,
    actor_id: Uuid,
    denied_actor_id: Uuid,
    customer_id: i64,
    supplier_id: i64,
    sale_a_id: i64,
    sale_b_id: i64,
    foreign_sale_id: i64,
    cash_session_id: i64,
}
async fn setup() -> Option<(PgPool, Fixtures)> {
    let url = env::var("CHIMPPOS_FINANCIAL_TEST_DATABASE_URL").ok()?;
    let json = env::var("CHIMPPOS_FINANCIAL_TEST_FIXTURES_JSON").ok()?;
    let db = PgPool::connect(&url)
        .await
        .expect("connect disposable financial test database");
    let name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&db)
        .await
        .expect("database name");
    assert!(
        name.starts_with("chimp_pos_finance_test"),
        "refusing non-disposable financial test database"
    );
    Some((
        db,
        serde_json::from_str(&json).expect("valid financial fixture JSON"),
    ))
}
fn d(v: &str) -> Decimal {
    Decimal::from_str(v).unwrap()
}
fn invoice_request(key: Uuid, currency: String, number: String) -> SupplierInvoiceCreate {
    SupplierInvoiceCreate {
        idempotency_key: key,
        invoice_number: number,
        supplier_reference: None,
        purchase_order_id: None,
        invoice_date: NaiveDate::from_ymd_opt(2026, 11, 12).unwrap(),
        due_date: None,
        currency_code: currency,
        additional_charges: d("2.00"),
        notes: None,
        lines: vec![SupplierInvoiceLineInput {
            description: "Integration test stock".into(),
            quantity: d("2"),
            unit_cost: d("10.00"),
            tax_amount: d("1.00"),
            discount_amount: d("0.50"),
            purchase_order_item_id: None,
            goods_receipt_item_id: None,
        }],
    }
}

#[tokio::test]
async fn posted_customer_and_supplier_financial_workflows_preserve_invariants() {
    let Some((db, f)) = setup().await else {
        eprintln!(
            "skipped: set CHIMPPOS_FINANCIAL_TEST_DATABASE_URL and CHIMPPOS_FINANCIAL_TEST_FIXTURES_JSON for a disposable PostgreSQL database"
        );
        return;
    };
    let small = d("0.01");

    // Input over-allocation is rejected before any payment row is written.
    let excessive = finance::post_customer_payment(
        &db,
        f.company_id,
        f.actor_id,
        f.customer_id,
        CustomerPaymentCreate {
            idempotency_key: Uuid::new_v4(),
            amount: small,
            payment_method: "card".into(),
            cash_session_id: None,
            reference: None,
            notes: None,
            allocations: vec![AllocationInput {
                document_id: f.sale_a_id,
                amount: d("0.02"),
            }],
        },
    )
    .await;
    assert!(excessive.is_err());

    // A receipt can be partially allocated across multiple sales and replayed safely.
    let customer_key = Uuid::new_v4();
    let request = CustomerPaymentCreate {
        idempotency_key: customer_key,
        amount: d("0.03"),
        payment_method: "cash".into(),
        cash_session_id: Some(f.cash_session_id),
        reference: Some("M2-test".into()),
        notes: None,
        allocations: vec![
            AllocationInput {
                document_id: f.sale_a_id,
                amount: small,
            },
            AllocationInput {
                document_id: f.sale_b_id,
                amount: small,
            },
        ],
    };
    let receipt =
        finance::post_customer_payment(&db, f.company_id, f.actor_id, f.customer_id, request)
            .await
            .unwrap();
    assert!(!receipt.replayed);
    let replay = finance::post_customer_payment(
        &db,
        f.company_id,
        f.actor_id,
        f.customer_id,
        CustomerPaymentCreate {
            idempotency_key: customer_key,
            amount: d("0.03"),
            payment_method: "cash".into(),
            cash_session_id: Some(f.cash_session_id),
            reference: Some("M2-test".into()),
            notes: None,
            allocations: vec![
                AllocationInput {
                    document_id: f.sale_b_id,
                    amount: small,
                },
                AllocationInput {
                    document_id: f.sale_a_id,
                    amount: small,
                },
            ],
        },
    )
    .await
    .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.id, receipt.id);
    let stored_session: Option<i64> = sqlx::query_scalar(
        "SELECT cash_session_id FROM customer_payments WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company_id)
    .bind(receipt.id)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(stored_session, Some(f.cash_session_id));
    let cash_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cash_movements WHERE company_id=$1 AND idempotency_key=$2",
    )
    .bind(f.company_id)
    .bind(customer_key)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(
        cash_rows, 0,
        "account receipt must not create a duplicate manual cash movement"
    );

    // A foreign-shop sale cannot be attached to this customer's receipt.
    let foreign = finance::post_customer_payment(
        &db,
        f.company_id,
        f.actor_id,
        f.customer_id,
        CustomerPaymentCreate {
            idempotency_key: Uuid::new_v4(),
            amount: small,
            payment_method: "card".into(),
            cash_session_id: None,
            reference: None,
            notes: None,
            allocations: vec![AllocationInput {
                document_id: f.foreign_sale_id,
                amount: small,
            }],
        },
    )
    .await;
    assert!(foreign.is_err());

    // Service permission check rejects an actor with no customer payment grant.
    let denied = finance::post_customer_payment(
        &db,
        f.company_id,
        f.denied_actor_id,
        f.customer_id,
        CustomerPaymentCreate {
            idempotency_key: Uuid::new_v4(),
            amount: small,
            payment_method: "card".into(),
            cash_session_id: None,
            reference: None,
            notes: None,
            allocations: vec![],
        },
    )
    .await;
    assert!(denied.is_err());

    let currency: String =
        sqlx::query_scalar("SELECT default_currency::text FROM companies WHERE id=$1")
            .bind(f.company_id)
            .fetch_one(&db)
            .await
            .unwrap();
    let invoice_key = Uuid::new_v4();
    let invoice = finance::create_supplier_invoice(
        &db,
        f.company_id,
        f.actor_id,
        f.supplier_id,
        invoice_request(
            invoice_key,
            currency.trim().into(),
            format!("M2-{}", Uuid::new_v4()),
        ),
    )
    .await
    .unwrap();
    assert_eq!(invoice.status, "posted");
    assert_eq!(invoice.total_amount, d("22.50"));
    assert_eq!(invoice.lines.len(), 1);
    let invoice_replay = finance::create_supplier_invoice(
        &db,
        f.company_id,
        f.actor_id,
        f.supplier_id,
        invoice_request(
            invoice_key,
            currency.trim().into(),
            invoice.invoice_number.clone(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(invoice_replay.id, invoice.id);

    // Supplier payment is partial and cash is recorded once against the actual session.
    let supplier_key = Uuid::new_v4();
    let payment = finance::post_supplier_payment(
        &db,
        f.company_id,
        f.actor_id,
        f.supplier_id,
        SupplierPaymentCreate {
            idempotency_key: supplier_key,
            amount: d("2.50"),
            payment_method: "cash".into(),
            cash_session_id: Some(f.cash_session_id),
            reference: None,
            notes: None,
            allocations: vec![AllocationInput {
                document_id: invoice.id,
                amount: d("2.50"),
            }],
        },
    )
    .await
    .unwrap();
    assert!(!payment.replayed);
    let stored_supplier_session: Option<i64> = sqlx::query_scalar(
        "SELECT cash_session_id FROM supplier_payments WHERE company_id=$1 AND id=$2",
    )
    .bind(f.company_id)
    .bind(payment.id)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(stored_supplier_session, Some(f.cash_session_id));

    // Base schema has no currency column on supplier payments; foreign-currency
    // invoice posting is rejected instead of silently creating a mixed ledger.
    let mismatch = finance::create_supplier_invoice(
        &db,
        f.company_id,
        f.actor_id,
        f.supplier_id,
        invoice_request(
            Uuid::new_v4(),
            "ZZZ".into(),
            format!("M2-CUR-{}", Uuid::new_v4()),
        ),
    )
    .await;
    assert!(mismatch.is_err());

    // No credit note can exceed the remaining invoice balance.
    let over_credit = finance::post_supplier_credit_note(
        &db,
        f.company_id,
        f.actor_id,
        f.supplier_id,
        CreditNoteCreate {
            idempotency_key: Uuid::new_v4(),
            document_id: Some(invoice.id),
            amount: d("1000"),
            reason: "over-credit test".into(),
        },
    )
    .await;
    assert!(over_credit.is_err());

    // Force a database error after draft header and line insertion. Both must roll back.
    sqlx::query("DROP TRIGGER IF EXISTS m2_test_reject_invoice_post ON supplier_invoices")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("CREATE OR REPLACE FUNCTION m2_test_reject_invoice_post_fn() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.invoice_number LIKE 'M2-ROLLBACK-%' AND NEW.status='posted' THEN RAISE EXCEPTION 'intentional milestone rollback test'; END IF; RETURN NEW; END $$").execute(&db).await.unwrap();
    sqlx::query("CREATE TRIGGER m2_test_reject_invoice_post BEFORE UPDATE ON supplier_invoices FOR EACH ROW EXECUTE FUNCTION m2_test_reject_invoice_post_fn()").execute(&db).await.unwrap();
    let rollback_key = Uuid::new_v4();
    let failed = finance::create_supplier_invoice(
        &db,
        f.company_id,
        f.actor_id,
        f.supplier_id,
        invoice_request(
            rollback_key,
            currency.trim().into(),
            format!("M2-ROLLBACK-{rollback_key}"),
        ),
    )
    .await;
    sqlx::query("DROP TRIGGER IF EXISTS m2_test_reject_invoice_post ON supplier_invoices")
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION IF EXISTS m2_test_reject_invoice_post_fn()")
        .execute(&db)
        .await
        .unwrap();
    assert!(failed.is_err());
    let leftovers: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM supplier_invoices WHERE company_id=$1 AND idempotency_key=$2",
    )
    .bind(f.company_id)
    .bind(rollback_key)
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(
        leftovers, 0,
        "failed invoice posting must roll back header and lines"
    );
    let _ = f.foreign_company_id;
}
