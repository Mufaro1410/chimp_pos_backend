//! Opt-in PostgreSQL integration scenarios. The configured database must be a
//! fresh disposable database with migrations and named fixtures already loaded.
//! This suite intentionally refuses `chimp_pos_dev` and does not run migrations.

use chimpPOS_backend::{
    models::{
        cash_register::CloseCashSessionRequest,
        financial_reversals::{AllocationReversalInput, PaymentReversalRequest},
    },
    services::{cash_register_service, financial_reversals_service, sales_service},
};
use rust_decimal::Decimal;
use serde::Deserialize;
use sqlx::{PgPool, Row};
use std::{env, str::FromStr};
use uuid::Uuid;

#[derive(Clone, Deserialize)]
struct Fixtures {
    company_id: Uuid,
    actor_id: Uuid,
    customer_id: i64,
    customer_credit_attempt: String,
    customer_allocation_payment_id: i64,
    customer_allocation_sale_a_id: i64,
    customer_allocation_sale_b_id: i64,
    supplier_allocation_payment_a_id: i64,
    supplier_allocation_payment_b_id: i64,
    supplier_allocation_invoice_id: i64,
    customer_reversal_payment_id: i64,
    customer_reversal_allocation_id: i64,
    customer_rollback_allocation_id: i64,
    supplier_reversal_payment_id: i64,
    supplier_reversal_allocation_id: i64,
    customer_credit_note_id: i64,
    customer_cash_payment_id: i64,
    cash_session_id: i64,
}

async fn setup() -> Option<(PgPool, Fixtures)> {
    let url = env::var("STAGE13B_TEST_DATABASE_URL").ok()?;
    let json = env::var("STAGE13B_TEST_FIXTURES_JSON").ok()?;
    let pool = PgPool::connect(&url)
        .await
        .expect("connect disposable Stage13B test DB");
    let db_name: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .expect("read database name");
    assert!(
        db_name.starts_with("chimp_pos_stage13b_test"),
        "refusing non-disposable database {db_name}"
    );
    Some((
        pool,
        serde_json::from_str(&json).expect("valid fixture JSON"),
    ))
}

async fn allocate_customer(
    pool: PgPool,
    f: Fixtures,
    sale_id: i64,
    amount: Decimal,
    barrier: std::sync::Arc<tokio::sync::Barrier>,
) -> bool {
    barrier.wait().await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM customer_payments WHERE id=$1 AND company_id=$2 FOR UPDATE")
        .bind(f.customer_allocation_payment_id)
        .bind(f.company_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM sales WHERE id=$1 AND company_id=$2 FOR UPDATE")
        .bind(sale_id)
        .bind(f.company_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let inserted = sqlx::query("INSERT INTO customer_payment_allocations(company_id,payment_id,sale_id,customer_id,currency_code,amount) SELECT p.company_id,p.id,$3,p.customer_id,p.currency_code,$4 FROM customer_payments p WHERE p.id=$1 AND p.company_id=$2")
        .bind(f.customer_allocation_payment_id).bind(f.company_id).bind(sale_id).bind(amount)
        .execute(&mut *tx).await;
    match inserted {
        Ok(_) => {
            tx.commit().await.unwrap();
            true
        }
        Err(_) => {
            tx.rollback().await.unwrap();
            false
        }
    }
}

async fn allocate_supplier(
    pool: PgPool,
    f: Fixtures,
    payment_id: i64,
    amount: Decimal,
    barrier: std::sync::Arc<tokio::sync::Barrier>,
) -> bool {
    barrier.wait().await;
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM supplier_payments WHERE id=$1 AND company_id=$2 FOR UPDATE")
        .bind(payment_id)
        .bind(f.company_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM supplier_invoices WHERE id=$1 AND company_id=$2 FOR UPDATE")
        .bind(f.supplier_allocation_invoice_id)
        .bind(f.company_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let inserted = sqlx::query("INSERT INTO supplier_payment_allocations(company_id,payment_id,invoice_id,supplier_id,currency_code,amount) SELECT p.company_id,p.id,$3,p.supplier_id,p.currency_code,$4 FROM supplier_payments p WHERE p.id=$1 AND p.company_id=$2")
        .bind(payment_id).bind(f.company_id).bind(f.supplier_allocation_invoice_id).bind(amount)
        .execute(&mut *tx).await;
    match inserted {
        Ok(_) => {
            tx.commit().await.unwrap();
            true
        }
        Err(_) => {
            tx.rollback().await.unwrap();
            false
        }
    }
}

#[tokio::test]
async fn postgres_financial_integrity_concurrency_reversal_idempotency_and_rollback() {
    let Some((pool, f)) = setup().await else {
        eprintln!(
            "skipped: set STAGE13B_TEST_DATABASE_URL and STAGE13B_TEST_FIXTURES_JSON for a disposable PostgreSQL database"
        );
        return;
    };
    let one_cent = Decimal::new(1, 2);

    // Two concurrent allocations must serialize on the payment row; only one
    // can consume a payment with one cent available.
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let a = tokio::spawn(allocate_customer(
        pool.clone(),
        f.clone(),
        f.customer_allocation_sale_a_id,
        one_cent,
        barrier.clone(),
    ));
    let b = tokio::spawn(allocate_customer(
        pool.clone(),
        f.clone(),
        f.customer_allocation_sale_b_id,
        one_cent,
        barrier,
    ));
    assert_ne!(a.await.unwrap(), b.await.unwrap());

    // Different payments contend for the same invoice's one-cent open amount.
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let a = tokio::spawn(allocate_supplier(
        pool.clone(),
        f.clone(),
        f.supplier_allocation_payment_a_id,
        one_cent,
        barrier.clone(),
    ));
    let b = tokio::spawn(allocate_supplier(
        pool.clone(),
        f.clone(),
        f.supplier_allocation_payment_b_id,
        one_cent,
        barrier,
    ));
    assert_ne!(a.await.unwrap(), b.await.unwrap());

    // Competing credit sales use the same customer lock and fresh balance read.
    let attempt = Decimal::from_str(&f.customer_credit_attempt).unwrap();
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let pool = pool.clone();
        let company = f.company_id;
        let customer = f.customer_id;
        let actor = f.actor_id;
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            let mut tx = pool.begin().await.unwrap();
            let permitted = sales_service::customer_credit_permits(&mut tx, company, customer, attempt).await.unwrap();
            if permitted {
                sqlx::query("INSERT INTO sales(company_id,location_id,sale_number,status,subtotal,total_amount,created_by,completed_by,completed_at,customer_id,currency_code) SELECT $1,l.id,(SELECT next_number FROM sale_sequences WHERE company_id=$1), 'completed',$2,$2,$3,$3,NOW(),c.id,c.currency_code FROM customers c JOIN inventory_locations l ON l.company_id=c.company_id WHERE c.id=$4 AND c.company_id=$1 LIMIT 1")
                    .bind(company).bind(attempt).bind(actor).bind(customer).execute(&mut *tx).await.unwrap();
                sqlx::query("UPDATE sale_sequences SET next_number=next_number+1 WHERE company_id=$1").bind(company).execute(&mut *tx).await.unwrap();
            }
            tx.commit().await.unwrap(); permitted
        }));
    }
    let credits = futures_join(tasks).await;
    assert_eq!(credits.iter().filter(|v| **v).count(), 1);

    // Allocation reversal and payment reversal are one transaction. Same key
    // and payload replays the committed reversal; a later invalid posting rolls
    // back any allocation reversal inserted earlier in that transaction.
    let allocation_key = Uuid::new_v4();
    let key = Uuid::new_v4();
    let request = PaymentReversalRequest {
        idempotency_key: key,
        amount: one_cent,
        payment_method: "card".into(),
        cash_session_id: None,
        reason: "integration reversal".into(),
        allocations: vec![AllocationReversalInput {
            allocation_id: f.customer_reversal_allocation_id,
            amount: one_cent,
            idempotency_key: allocation_key,
        }],
    };
    let first = financial_reversals_service::reverse_customer_payment(
        &pool,
        f.company_id,
        f.actor_id,
        f.customer_reversal_payment_id,
        request,
    )
    .await
    .unwrap();
    assert!(!first.replayed);
    let replay = financial_reversals_service::reverse_customer_payment(
        &pool,
        f.company_id,
        f.actor_id,
        f.customer_reversal_payment_id,
        PaymentReversalRequest {
            idempotency_key: key,
            amount: one_cent,
            payment_method: "card".into(),
            cash_session_id: None,
            reason: "integration reversal".into(),
            allocations: vec![AllocationReversalInput {
                allocation_id: f.customer_reversal_allocation_id,
                amount: one_cent,
                idempotency_key: allocation_key,
            }],
        },
    )
    .await
    .unwrap();
    assert!(replay.replayed);

    let over_allocation_reversal = sqlx::query("INSERT INTO customer_allocation_reversals(company_id,allocation_id,payment_id,sale_id,customer_id,currency_code,amount,reason,idempotency_key,request_fingerprint,reversed_by) SELECT a.company_id,a.id,a.payment_id,a.sale_id,a.customer_id,a.currency_code,a.amount-COALESCE(SUM(r.amount),0)+0.0001,'must exceed allocation',gen_random_uuid(),'allocation-over-cap-test',$3 FROM customer_payment_allocations a LEFT JOIN customer_allocation_reversals r ON r.allocation_id=a.id AND r.company_id=a.company_id WHERE a.company_id=$1 AND a.id=$2 GROUP BY a.company_id,a.id,a.payment_id,a.sale_id,a.customer_id,a.currency_code,a.amount")
        .bind(f.company_id).bind(f.customer_reversal_allocation_id).bind(f.actor_id).execute(&pool).await;
    assert!(over_allocation_reversal.is_err());

    let failed_key = Uuid::new_v4();
    let failed_alloc_key = Uuid::new_v4();
    let failed = financial_reversals_service::reverse_customer_payment(
        &pool,
        f.company_id,
        f.actor_id,
        f.customer_reversal_payment_id,
        PaymentReversalRequest {
            idempotency_key: failed_key,
            amount: Decimal::from(1_000_000),
            payment_method: "card".into(),
            cash_session_id: None,
            reason: "rollback integration".into(),
            allocations: vec![AllocationReversalInput {
                allocation_id: f.customer_rollback_allocation_id,
                amount: one_cent,
                idempotency_key: failed_alloc_key,
            }],
        },
    )
    .await;
    assert!(failed.is_err());
    let rolled_back: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM customer_allocation_reversals WHERE company_id=$1 AND idempotency_key=$2)")
        .bind(f.company_id).bind(failed_alloc_key).fetch_one(&pool).await.unwrap();
    assert!(rolled_back);

    let supplier_reversal = financial_reversals_service::reverse_supplier_payment(
        &pool,
        f.company_id,
        f.actor_id,
        f.supplier_reversal_payment_id,
        PaymentReversalRequest {
            idempotency_key: Uuid::new_v4(),
            amount: one_cent,
            payment_method: "card".into(),
            cash_session_id: None,
            reason: "supplier reversal integration".into(),
            allocations: vec![AllocationReversalInput {
                allocation_id: f.supplier_reversal_allocation_id,
                amount: one_cent,
                idempotency_key: Uuid::new_v4(),
            }],
        },
    )
    .await
    .unwrap();
    assert!(!supplier_reversal.replayed);

    let credit_reversal = financial_reversals_service::reverse_credit_note(
        &pool,
        f.company_id,
        f.actor_id,
        f.customer_credit_note_id,
        chimpPOS_backend::models::financial_reversals::CreditNoteReversalRequest {
            idempotency_key: Uuid::new_v4(),
            amount: one_cent,
            reason: "credit note correction".into(),
        },
        true,
    )
    .await
    .unwrap();
    assert!(!credit_reversal.replayed);

    // Cash close and cash reversal race. Whichever wins the session lock, the
    // reversal is either included exactly once or rejected against a closed session.
    let reverse_pool = pool.clone();
    let close_pool = pool.clone();
    let company = f.company_id;
    let actor = f.actor_id;
    let payment = f.customer_cash_payment_id;
    let session = f.cash_session_id;
    let reverse = tokio::spawn(async move {
        financial_reversals_service::reverse_customer_payment(
            &reverse_pool,
            company,
            actor,
            payment,
            PaymentReversalRequest {
                idempotency_key: Uuid::new_v4(),
                amount: one_cent,
                payment_method: "cash".into(),
                cash_session_id: Some(session),
                reason: "close race".into(),
                allocations: vec![],
            },
        )
        .await
    });
    let close = tokio::spawn(async move {
        cash_register_service::close_session(
            &close_pool,
            company,
            actor,
            session,
            CloseCashSessionRequest {
                closing_cash_count: Decimal::from(100000),
                closing_notes: Some("integration close race".into()),
            },
        )
        .await
    });
    let _ = tokio::join!(reverse, close);
    let consistency: bool = sqlx::query_scalar("SELECT expected_cash = opening_float +(SELECT COALESCE(SUM(amount),0) FROM sale_payments WHERE cash_session_id=s.id AND company_id=s.company_id AND payment_method='cash') -(SELECT COALESCE(SUM(amount),0) FROM sale_refunds WHERE cash_session_id=s.id AND company_id=s.company_id AND payment_method='cash') +(SELECT COALESCE(SUM(amount),0) FROM customer_payments WHERE cash_session_id=s.id AND company_id=s.company_id AND payment_method='cash') -(SELECT COALESCE(SUM(amount),0) FROM supplier_payments WHERE cash_session_id=s.id AND company_id=s.company_id AND payment_method='cash') -(SELECT COALESCE(SUM(amount),0) FROM customer_payment_reversals WHERE cash_session_id=s.id AND company_id=s.company_id AND payment_method='cash') +(SELECT COALESCE(SUM(amount),0) FROM supplier_payment_reversals WHERE cash_session_id=s.id AND company_id=s.company_id AND payment_method='cash') +(SELECT COALESCE(SUM(amount) FILTER(WHERE movement_type='cash_in'),0)-COALESCE(SUM(amount) FILTER(WHERE movement_type IN ('cash_out','paid_out','cash_drop')),0) FROM cash_movements WHERE register_session_id=s.id AND company_id=s.company_id) FROM cash_register_sessions s WHERE id=$1 AND company_id=$2 AND status='closed'")
        .bind(session).bind(f.company_id).fetch_one(&pool).await.unwrap();
    assert!(consistency);

    // Ensure the configured test fixture includes supplier reversal coverage;
    // the service is exercised on the same opt-in database by the paired test.
    let supplier_row =
        sqlx::query("SELECT id FROM supplier_payments WHERE id=$1 AND company_id=$2")
            .bind(f.supplier_reversal_payment_id)
            .bind(f.company_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(supplier_row.try_get::<i64, _>("id").is_ok());
    let _ = f.supplier_reversal_allocation_id;
}

async fn futures_join<T>(tasks: Vec<tokio::task::JoinHandle<T>>) -> Vec<T> {
    let mut out = Vec::with_capacity(tasks.len());
    for task in tasks {
        out.push(task.await.unwrap());
    }
    out
}
