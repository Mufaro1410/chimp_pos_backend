use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::str::FromStr;
use uuid::Uuid;

use crate::models::reporting::ReportQuery;

#[derive(Debug)]
pub enum ReportingError {
    Invalid(&'static str),
    Database(sqlx::Error),
    NotFound,
}
impl From<sqlx::Error> for ReportingError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

fn bounds(q: &ReportQuery) -> Result<(DateTime<Utc>, DateTime<Utc>), ReportingError> {
    let today = Utc::now().date_naive();
    let from = q.from.unwrap_or(today);
    let to = q.to.unwrap_or(from);
    if to < from || (to - from).num_days() > 3660 {
        return Err(ReportingError::Invalid(
            "Date range is invalid or exceeds ten years",
        ));
    }
    let end_date = to
        .succ_opt()
        .ok_or(ReportingError::Invalid("Date range is invalid"))?;
    Ok((
        from.and_hms_opt(0, 0, 0).unwrap().and_utc(),
        end_date.and_hms_opt(0, 0, 0).unwrap().and_utc(),
    ))
}
fn paging(q: &ReportQuery) -> (i64, i64) {
    (
        q.limit.unwrap_or(50).clamp(1, 100),
        q.offset.unwrap_or(0).max(0),
    )
}
fn threshold(q: &ReportQuery) -> Result<Option<Decimal>, ReportingError> {
    let Some(value) = q.threshold.as_ref() else {
        return Ok(None);
    };
    let parsed = Decimal::from_str(value)
        .map_err(|_| ReportingError::Invalid("Stock threshold must be a nonnegative decimal"))?;
    if parsed < Decimal::ZERO || parsed.scale() > 4 {
        return Err(ReportingError::Invalid(
            "Stock threshold must be a nonnegative decimal with at most four fractional digits",
        ));
    }
    Ok(Some(parsed))
}
async fn value(db: &PgPool, sql: &'static str, company: Uuid) -> Result<Value, ReportingError> {
    Ok(sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .fetch_one(db)
        .await?)
}
async fn company_currency(db: &PgPool, company: Uuid) -> Result<String, ReportingError> {
    Ok(
        sqlx::query_scalar("SELECT default_currency::text FROM companies WHERE id=$1")
            .bind(company)
            .fetch_optional(db)
            .await?
            .ok_or(ReportingError::NotFound)?,
    )
}

pub async fn dashboard(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let low_threshold = threshold(q)?;
    let sql = r#"WITH local_day AS (
      SELECT timezone(c.timezone, now())::date AS local_date,c.timezone shop_timezone FROM companies c WHERE c.id=$1
    ), sales_today AS (
      SELECT s.id,s.total_amount FROM sales s,local_day d
      WHERE s.company_id=$1 AND s.status='completed' AND timezone(d.shop_timezone,s.completed_at)::date=d.local_date
    ), payment_methods AS (
      SELECT p.payment_method, SUM(p.amount) amount FROM sale_payments p JOIN sales s
        ON s.id=p.sale_id AND s.company_id=p.company_id,local_day d
      WHERE s.company_id=$1 AND s.status='completed' AND timezone(d.shop_timezone,p.created_at)::date=d.local_date
      GROUP BY p.payment_method
    ), stock AS (
      SELECT sb.variant_id,SUM(sb.quantity_on_hand) quantity FROM stock_balances sb
      WHERE sb.company_id=$1 GROUP BY sb.variant_id
    )
    SELECT jsonb_build_object(
      'date', (SELECT local_date::text FROM local_day), 'currency_code', (SELECT default_currency::text FROM companies WHERE id=$1),
      'sales_total', COALESCE((SELECT SUM(total_amount)::text FROM sales_today),'0'),
      'sales_transaction_count', (SELECT COUNT(*) FROM sales_today),
      'average_transaction_value', COALESCE((SELECT AVG(total_amount)::text FROM sales_today),'0'),
      'sales_by_payment_method', COALESCE((SELECT jsonb_agg(jsonb_build_object('payment_method',payment_method,'amount',amount::text) ORDER BY payment_method) FROM payment_methods),'[]'::jsonb),
      'low_stock_threshold', $2::text,
      'low_stock_product_count', CASE WHEN $2::numeric IS NULL THEN NULL ELSE (
        SELECT COUNT(DISTINCT p.id) FROM products p JOIN product_variants v ON v.product_id=p.id AND v.company_id=p.company_id
        JOIN stock x ON x.variant_id=v.id WHERE p.company_id=$1 AND p.is_active AND v.is_active AND x.quantity>0 AND x.quantity<=$2::numeric
      ) END,
      'out_of_stock_product_count', (SELECT COUNT(*) FROM (
        SELECT p.id FROM products p JOIN product_variants v ON v.product_id=p.id AND v.company_id=p.company_id
        LEFT JOIN stock x ON x.variant_id=v.id WHERE p.company_id=$1 AND p.is_active AND v.is_active
        GROUP BY p.id HAVING COALESCE(SUM(x.quantity),0)<=0
      ) o),
      'total_products', (SELECT COUNT(*) FROM products WHERE company_id=$1),
      'active_products', (SELECT COUNT(*) FROM products WHERE company_id=$1 AND is_active),
      'recent_sales', COALESCE((SELECT jsonb_agg(jsonb_build_object('id',id,'sale_number',sale_number,'status',status,'total_amount',total_amount::text,'completed_at',completed_at) ORDER BY completed_at DESC,id DESC) FROM (SELECT id,sale_number,status,total_amount,completed_at FROM sales WHERE company_id=$1 AND status='completed' ORDER BY completed_at DESC,id DESC LIMIT 10) s),'[]'::jsonb),
      'recent_stock_movements', COALESCE((SELECT jsonb_agg(jsonb_build_object('id',m.id,'movement_type',m.movement_type,'quantity_delta',m.quantity_delta::text,'variant_id',m.variant_id,'location_id',m.location_id,'created_at',m.created_at) ORDER BY m.created_at DESC,m.id DESC) FROM (SELECT id,movement_type,quantity_delta,variant_id,location_id,created_at FROM stock_movements WHERE company_id=$1 ORDER BY created_at DESC,id DESC LIMIT 10) m),'[]'::jsonb),
      'cash_registers', COALESCE((SELECT jsonb_agg(jsonb_build_object('register_id',r.id,'code',r.code,'name',r.name,'is_active',r.is_active,'session_id',s.id,'session_status',s.status,'opened_at',s.opened_at) ORDER BY r.id) FROM cash_registers r LEFT JOIN LATERAL (SELECT id,status,opened_at FROM cash_register_sessions WHERE company_id=r.company_id AND register_id=r.id AND status='open' ORDER BY opened_at DESC LIMIT 1) s ON true WHERE r.company_id=$1),'[]'::jsonb)
    )"#;
    Ok(sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .bind(low_threshold.map(|v| v.to_string()))
        .fetch_one(db)
        .await?)
}

pub async fn sales_summary(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let interval = q.interval.as_deref().unwrap_or("day");
    if !["day", "week", "month"].contains(&interval) {
        return Err(ReportingError::Invalid(
            "interval must be day, week, or month",
        ));
    }
    let sql = r#"WITH selected AS (SELECT s.id,s.total_amount,s.completed_at FROM sales s WHERE s.company_id=$1 AND s.status='completed' AND s.completed_at >= $2 AND s.completed_at < $3), returns AS (SELECT r.sale_id,SUM(ri.refundable_amount) amount FROM sale_returns r JOIN sale_return_items ri ON ri.return_id=r.id AND ri.company_id=r.company_id WHERE r.company_id=$1 AND r.created_at >= $2 AND r.created_at < $3 GROUP BY r.sale_id), daily AS (SELECT date_trunc($4,s.completed_at)::date period,SUM(s.total_amount) gross,COUNT(*)::bigint transactions FROM selected s GROUP BY 1), refunded AS (SELECT COALESCE(SUM(amount),0) total FROM sale_refunds WHERE company_id=$1 AND created_at >= $2 AND created_at < $3), returned AS (SELECT COALESCE(SUM(amount),0) total FROM returns) SELECT jsonb_build_object('currency_code',(SELECT default_currency::text FROM companies WHERE id=$1),'from',($2 AT TIME ZONE 'UTC')::date::text,'to',(($3 AT TIME ZONE 'UTC')::date-1)::text,'interval',$4,'gross_sales',COALESCE((SELECT SUM(total_amount)::text FROM selected),'0'),'returns',COALESCE((SELECT total::text FROM returned),'0'),'refunds_paid',COALESCE((SELECT total::text FROM refunded),'0'),'net_sales',((COALESCE((SELECT SUM(total_amount) FROM selected),0)-COALESCE((SELECT total FROM returned),0))::text),'transaction_count',(SELECT COUNT(*) FROM selected),'average_transaction_value',COALESCE((SELECT AVG(total_amount)::text FROM selected),'0'),'breakdown',COALESCE((SELECT jsonb_agg(jsonb_build_object('period',period::text,'gross_sales',gross::text,'transaction_count',transactions) ORDER BY period) FROM daily),'[]'::jsonb))"#;
    Ok(sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .bind(from)
        .bind(to)
        .bind(interval)
        .fetch_one(db)
        .await?)
}

pub async fn sales_by_product(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let (limit, offset) = paging(q);
    let sql = r#"WITH sold AS (SELECT si.variant_id,MIN(si.product_name) product_name,MIN(si.variant_name) variant_name,MIN(si.sku) sku,SUM(si.quantity) quantity,SUM(si.line_total) gross FROM sale_items si JOIN sales s ON s.id=si.sale_id AND s.company_id=si.company_id WHERE si.company_id=$1 AND s.status='completed' AND s.completed_at >= $2 AND s.completed_at < $3 GROUP BY si.variant_id), returned AS (SELECT ri.variant_id,SUM(ri.quantity) quantity,SUM(ri.refundable_amount) amount FROM sale_returns r JOIN sale_return_items ri ON ri.return_id=r.id AND ri.company_id=r.company_id WHERE r.company_id=$1 AND r.created_at >= $2 AND r.created_at < $3 GROUP BY ri.variant_id) SELECT COALESCE(jsonb_agg(jsonb_build_object('variant_id',s.variant_id,'product_name',s.product_name,'variant_name',s.variant_name,'sku',s.sku,'quantity_sold',s.quantity::text,'quantity_returned',COALESCE(r.quantity,0)::text,'gross_sales',s.gross::text,'net_sales',(s.gross-COALESCE(r.amount,0))::text) ORDER BY s.gross DESC),'[]'::jsonb) FROM (SELECT * FROM sold ORDER BY gross DESC OFFSET $4 LIMIT $5) s LEFT JOIN returned r USING(variant_id)"#;
    let items = sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .bind(from)
        .bind(to)
        .bind(offset)
        .bind(limit)
        .fetch_one(db)
        .await?;
    Ok(
        json!({"currency_code":company_currency(db,company).await?,"items":items,"limit":limit,"offset":offset}),
    )
}

pub async fn sales_by_category(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let (limit, offset) = paging(q);
    let sql = r#"SELECT COALESCE(jsonb_agg(jsonb_build_object('category_id',category_id,'category_name',category_name,'gross_sales',gross::text,'transaction_count',transactions) ORDER BY gross DESC),'[]'::jsonb) FROM (SELECT c.id category_id,c.name category_name,SUM(si.line_total) gross,COUNT(DISTINCT s.id)::bigint transactions FROM sale_items si JOIN sales s ON s.id=si.sale_id AND s.company_id=si.company_id JOIN product_variants v ON v.id=si.variant_id AND v.company_id=si.company_id JOIN products p ON p.id=v.product_id AND p.company_id=v.company_id LEFT JOIN categories c ON c.id=p.category_id AND c.company_id=p.company_id WHERE s.company_id=$1 AND s.status='completed' AND s.completed_at >= $2 AND s.completed_at < $3 GROUP BY c.id,c.name ORDER BY gross DESC OFFSET $4 LIMIT $5) x"#;
    let items = sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .bind(from)
        .bind(to)
        .bind(offset)
        .bind(limit)
        .fetch_one(db)
        .await?;
    Ok(
        json!({"currency_code":company_currency(db,company).await?,"items":items,"limit":limit,"offset":offset}),
    )
}

pub async fn sales_by_payment_method(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let sql = r#"SELECT COALESCE(jsonb_agg(jsonb_build_object('payment_method',payment_method,'payments',payment_count,'amount',amount::text) ORDER BY payment_method),'[]'::jsonb) FROM (SELECT p.payment_method,COUNT(*)::bigint payment_count,SUM(p.amount) amount FROM sale_payments p JOIN sales s ON s.id=p.sale_id AND s.company_id=p.company_id WHERE p.company_id=$1 AND s.status='completed' AND p.created_at >= $2 AND p.created_at < $3 GROUP BY p.payment_method) x"#;
    let items = sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .bind(from)
        .bind(to)
        .fetch_one(db)
        .await?;
    Ok(json!({"currency_code":company_currency(db,company).await?,"items":items}))
}

pub async fn sales_transactions(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let (limit, offset) = paging(q);
    if q.status
        .as_deref()
        .is_some_and(|s| !["draft", "completed", "cancelled"].contains(&s))
    {
        return Err(ReportingError::Invalid("Invalid sale status"));
    }
    if q.payment_method
        .as_deref()
        .is_some_and(|m| !["cash", "card", "mobile_money", "other"].contains(&m))
    {
        return Err(ReportingError::Invalid("Invalid payment method"));
    }
    let rows=sqlx::query(r#"SELECT jsonb_build_object('id',s.id,'sale_number',s.sale_number,'status',s.status,'location_id',s.location_id,'subtotal',s.subtotal::text,'discount_total',s.discount_total::text,'tax_total',s.tax_total::text,'total_amount',s.total_amount::text,'created_at',s.created_at,'completed_at',s.completed_at,'currency_code',(SELECT default_currency::text FROM companies WHERE id=s.company_id),'voided',EXISTS(SELECT 1 FROM sale_voids v WHERE v.company_id=s.company_id AND v.sale_id=s.id),'refund_total',COALESCE((SELECT SUM(r.amount) FROM sale_refunds r WHERE r.company_id=s.company_id AND r.sale_id=s.id),0)::text,'payments',COALESCE((SELECT jsonb_agg(jsonb_build_object('method',p.payment_method,'amount',p.amount::text) ORDER BY p.id) FROM sale_payments p WHERE p.company_id=s.company_id AND p.sale_id=s.id),'[]'::jsonb)) row FROM sales s WHERE s.company_id=$1 AND s.created_at >= $2 AND s.created_at < $3 AND ($4::text IS NULL OR s.status=$4) AND ($5::bigint IS NULL OR s.location_id=$5) AND ($6::text IS NULL OR EXISTS(SELECT 1 FROM sale_payments p WHERE p.company_id=s.company_id AND p.sale_id=s.id AND p.payment_method=$6)) ORDER BY s.created_at DESC,s.id DESC LIMIT $7 OFFSET $8"#).bind(company).bind(from).bind(to).bind(&q.status).bind(q.location_id).bind(&q.payment_method).bind(limit).bind(offset).fetch_all(db).await?;
    Ok(
        json!({"items":rows.into_iter().map(|r|r.try_get::<Value,_>("row")).collect::<Result<Vec<_>,_>>()?,"limit":limit,"offset":offset,"from":from.date_naive().to_string(),"to":(to.date_naive()-Duration::days(1)).to_string()}),
    )
}

pub async fn sales_returns(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let sql = r#"SELECT jsonb_build_object('currency_code',(SELECT default_currency::text FROM companies WHERE id=$1),'returns',jsonb_build_object('count',(SELECT COUNT(*) FROM sale_returns WHERE company_id=$1 AND created_at >= $2 AND created_at < $3),'value',COALESCE((SELECT SUM(ri.refundable_amount)::text FROM sale_returns r JOIN sale_return_items ri ON ri.return_id=r.id AND ri.company_id=r.company_id WHERE r.company_id=$1 AND r.created_at >= $2 AND r.created_at < $3),'0')),'refunds',jsonb_build_object('count',(SELECT COUNT(*) FROM sale_refunds WHERE company_id=$1 AND created_at >= $2 AND created_at < $3),'value',COALESCE((SELECT SUM(amount)::text FROM sale_refunds WHERE company_id=$1 AND created_at >= $2 AND created_at < $3),'0')),'voids',jsonb_build_object('count',(SELECT COUNT(*) FROM sale_voids WHERE company_id=$1 AND created_at >= $2 AND created_at < $3),'value',COALESCE((SELECT SUM(s.total_amount)::text FROM sale_voids v JOIN sales s ON s.id=v.sale_id AND s.company_id=v.company_id WHERE v.company_id=$1 AND v.created_at >= $2 AND v.created_at < $3),'0')),'from',($2 AT TIME ZONE 'UTC')::date::text,'to',(($3 AT TIME ZONE 'UTC')::date-1)::text)"#;
    Ok(sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .bind(from)
        .bind(to)
        .fetch_one(db)
        .await?)
}

pub async fn inventory_stock(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (limit, offset) = paging(q);
    let threshold = threshold(q)?;
    let status = q.status.as_deref().unwrap_or("all");
    if !["all", "low", "out"].contains(&status) {
        return Err(ReportingError::Invalid("status must be all, low, or out"));
    }
    if status == "low" && threshold.is_none() {
        return Err(ReportingError::Invalid(
            "low stock report requires an explicit threshold",
        ));
    }
    let sql = r#"WITH stock AS (SELECT p.id product_id,p.name product_name,p.is_active,v.id variant_id,v.name variant_name,v.sku,v.cost_price,SUM(sb.quantity_on_hand) quantity_on_hand,SUM(sb.reserved_quantity) reserved_quantity FROM products p JOIN product_variants v ON v.product_id=p.id AND v.company_id=p.company_id LEFT JOIN stock_balances sb ON sb.variant_id=v.id AND sb.company_id=v.company_id AND ($2::bigint IS NULL OR sb.location_id=$2) WHERE p.company_id=$1 GROUP BY p.id,p.name,p.is_active,v.id,v.name,v.sku,v.cost_price) SELECT COALESCE(jsonb_agg(jsonb_build_object('product_id',product_id,'product_name',product_name,'product_active',is_active,'variant_id',variant_id,'variant_name',variant_name,'sku',sku,'quantity_on_hand',COALESCE(quantity_on_hand,0)::text,'reserved_quantity',COALESCE(reserved_quantity,0)::text,'available_quantity',(COALESCE(quantity_on_hand,0)-COALESCE(reserved_quantity,0))::text,'cost_price',cost_price::text) ORDER BY product_name,variant_name),'[]'::jsonb) FROM (SELECT * FROM stock WHERE ($3='all' OR ($3='out' AND COALESCE(quantity_on_hand,0)<=0) OR ($3='low' AND COALESCE(quantity_on_hand,0)>0 AND COALESCE(quantity_on_hand,0)<=$4::numeric)) ORDER BY product_name,variant_name OFFSET $5 LIMIT $6) x"#;
    let items = sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .bind(q.location_id)
        .bind(status)
        .bind(threshold.map(|v| v.to_string()))
        .bind(offset)
        .bind(limit)
        .fetch_one(db)
        .await?;
    Ok(
        json!({"currency_code":company_currency(db,company).await?,"items":items,"limit":limit,"offset":offset}),
    )
}

pub async fn inventory_valuation(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let sql = r#"SELECT jsonb_build_object('valuation_method','current variant cost_price × current on-hand quantity','currency_code',c.default_currency::text,'valued_stock',COALESCE(SUM(sb.quantity_on_hand*v.cost_price) FILTER(WHERE v.cost_price IS NOT NULL),0)::text,'uncosted_quantity',COALESCE(SUM(sb.quantity_on_hand) FILTER(WHERE v.cost_price IS NULL),0)::text,'uncosted_variant_count',COUNT(DISTINCT v.id) FILTER(WHERE v.cost_price IS NULL AND sb.quantity_on_hand>0),'as_of',now()) FROM companies c LEFT JOIN stock_balances sb ON sb.company_id=c.id AND ($2::bigint IS NULL OR sb.location_id=$2) LEFT JOIN product_variants v ON v.id=sb.variant_id AND v.company_id=sb.company_id WHERE c.id=$1 GROUP BY c.default_currency"#;
    let _ = q;
    Ok(sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .bind(q.location_id)
        .fetch_one(db)
        .await?)
}

pub async fn inventory_movements(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let (limit, offset) = paging(q);
    let rows=sqlx::query(r#"SELECT jsonb_build_object('id',m.id,'location_id',m.location_id,'variant_id',m.variant_id,'product_name',p.name,'variant_name',v.name,'movement_type',m.movement_type,'quantity_delta',m.quantity_delta::text,'reference_type',m.reference_type,'reference_id',m.reference_id,'created_at',m.created_at) row FROM stock_movements m JOIN product_variants v ON v.id=m.variant_id AND v.company_id=m.company_id JOIN products p ON p.id=v.product_id AND p.company_id=v.company_id WHERE m.company_id=$1 AND m.created_at >= $2 AND m.created_at < $3 AND ($4::bigint IS NULL OR m.location_id=$4) AND ($5::bigint IS NULL OR m.variant_id=$5) ORDER BY m.created_at DESC,m.id DESC LIMIT $6 OFFSET $7"#).bind(company).bind(from).bind(to).bind(q.location_id).bind(q.variant_id).bind(limit).bind(offset).fetch_all(db).await?;
    Ok(
        json!({"items":rows.into_iter().map(|r|r.try_get::<Value,_>("row")).collect::<Result<Vec<_>,_>>()?,"limit":limit,"offset":offset}),
    )
}

pub async fn inventory_activity(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let sql = r#"SELECT jsonb_build_object('received_quantity',COALESCE(SUM(quantity_delta) FILTER(WHERE movement_type='purchase_receipt'),0)::text,'sold_quantity',COALESCE(-SUM(quantity_delta) FILTER(WHERE movement_type='sale'),0)::text,'from',($2 AT TIME ZONE 'UTC')::date::text,'to',(($3 AT TIME ZONE 'UTC')::date-1)::text) FROM stock_movements WHERE company_id=$1 AND created_at >= $2 AND created_at < $3 AND ($4::bigint IS NULL OR location_id=$4)"#;
    Ok(sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .bind(from)
        .bind(to)
        .bind(q.location_id)
        .fetch_one(db)
        .await?)
}

pub async fn purchasing_summary(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let sql = r#"SELECT jsonb_build_object('currency_code',(SELECT default_currency::text FROM companies WHERE id=$1),'purchase_orders',jsonb_build_object('count',(SELECT COUNT(*) FROM purchase_orders WHERE company_id=$1 AND order_date >= ($2 AT TIME ZONE 'UTC')::date AND order_date < ($3 AT TIME ZONE 'UTC')::date),'value',COALESCE((SELECT SUM(total_amount)::text FROM purchase_orders WHERE company_id=$1 AND order_date >= ($2 AT TIME ZONE 'UTC')::date AND order_date < ($3 AT TIME ZONE 'UTC')::date AND status<>'cancelled'),'0')),'goods_receipts',jsonb_build_object('count',(SELECT COUNT(*) FROM goods_receipts WHERE company_id=$1 AND received_at >= $2 AND received_at < $3),'quantity',COALESCE((SELECT SUM(ri.quantity_received)::text FROM goods_receipts gr JOIN goods_receipt_items ri ON ri.goods_receipt_id=gr.id AND ri.company_id=gr.company_id WHERE gr.company_id=$1 AND gr.received_at >= $2 AND gr.received_at < $3),'0')),'posted_supplier_invoices',jsonb_build_object('count',(SELECT COUNT(*) FROM supplier_invoices WHERE company_id=$1 AND status='posted' AND invoice_date >= ($2 AT TIME ZONE 'UTC')::date AND invoice_date < ($3 AT TIME ZONE 'UTC')::date),'by_currency',COALESCE((SELECT jsonb_agg(jsonb_build_object('currency_code',currency_code,'amount',amount::text) ORDER BY currency_code) FROM (SELECT btrim(currency_code)::text currency_code,SUM(total_amount) amount FROM supplier_invoices WHERE company_id=$1 AND status='posted' AND invoice_date >= ($2 AT TIME ZONE 'UTC')::date AND invoice_date < ($3 AT TIME ZONE 'UTC')::date GROUP BY btrim(currency_code)) currencies),'[]'::jsonb)),'from',($2 AT TIME ZONE 'UTC')::date::text,'to',(($3 AT TIME ZONE 'UTC')::date-1)::text)"#;
    Ok(sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .bind(from)
        .bind(to)
        .fetch_one(db)
        .await?)
}

pub async fn purchasing_by_supplier(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let (limit, offset) = paging(q);
    let sql = r#"WITH po AS (
      SELECT supplier_id,company_id,COUNT(*)::bigint order_count,SUM(total_amount) order_value
      FROM purchase_orders WHERE company_id=$1 AND order_date >= ($2 AT TIME ZONE 'UTC')::date AND order_date < ($3 AT TIME ZONE 'UTC')::date AND status<>'cancelled'
      GROUP BY supplier_id,company_id
    ), receipts AS (
      SELECT po.supplier_id,po.company_id,COUNT(DISTINCT gr.id)::bigint receipt_count,COALESCE(SUM(ri.quantity_received),0) received_quantity
      FROM purchase_orders po JOIN goods_receipts gr ON gr.purchase_order_id=po.id AND gr.company_id=po.company_id
      JOIN goods_receipt_items ri ON ri.goods_receipt_id=gr.id AND ri.company_id=gr.company_id
      WHERE po.company_id=$1 AND gr.received_at >= $2 AND gr.received_at < $3 GROUP BY po.supplier_id,po.company_id
    ), invoice_currency AS (
      SELECT supplier_id,company_id,btrim(currency_code)::text currency_code,SUM(total_amount) amount FROM supplier_invoices
      WHERE company_id=$1 AND status='posted' AND invoice_date >= ($2 AT TIME ZONE 'UTC')::date AND invoice_date < ($3 AT TIME ZONE 'UTC')::date GROUP BY supplier_id,company_id,btrim(currency_code)
    ), invoices AS (
      SELECT supplier_id,company_id,jsonb_agg(jsonb_build_object('currency_code',currency_code,'amount',amount::text) ORDER BY currency_code) totals FROM invoice_currency GROUP BY supplier_id,company_id
    ), combined AS (
      SELECT s.id supplier_id,s.name supplier_name,COALESCE(po.order_count,0) order_count,COALESCE(po.order_value,0) order_value,
        COALESCE(receipts.receipt_count,0) receipt_count,COALESCE(receipts.received_quantity,0) received_quantity,COALESCE(invoices.totals,'[]'::jsonb) invoice_totals
      FROM suppliers s LEFT JOIN po ON po.supplier_id=s.id AND po.company_id=s.company_id
      LEFT JOIN receipts ON receipts.supplier_id=s.id AND receipts.company_id=s.company_id
      LEFT JOIN invoices ON invoices.supplier_id=s.id AND invoices.company_id=s.company_id WHERE s.company_id=$1
    )
    SELECT COALESCE(jsonb_agg(jsonb_build_object('supplier_id',supplier_id,'supplier_name',supplier_name,'purchase_order_count',order_count,'purchase_order_value',order_value::text,'purchase_order_currency_code',(SELECT default_currency::text FROM companies WHERE id=$1),'goods_receipt_count',receipt_count,'received_quantity',received_quantity::text,'posted_invoice_totals',invoice_totals) ORDER BY order_value DESC),'[]'::jsonb)
    FROM (SELECT * FROM combined ORDER BY order_value DESC OFFSET $4 LIMIT $5) x"#;
    let items = sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .bind(from)
        .bind(to)
        .bind(offset)
        .bind(limit)
        .fetch_one(db)
        .await?;
    Ok(json!({"items":items,"limit":limit,"offset":offset}))
}

pub async fn goods_received(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let (limit, offset) = paging(q);
    let rows=sqlx::query(r#"SELECT jsonb_build_object('receipt_id',gr.id,'receipt_number',gr.receipt_number,'purchase_order_id',gr.purchase_order_id,'order_number',po.order_number,'supplier_id',po.supplier_id,'supplier_name',sp.name,'location_id',gr.location_id,'received_at',gr.received_at,'received_by',gr.received_by,'item_count',(SELECT COUNT(*) FROM goods_receipt_items ri WHERE ri.company_id=gr.company_id AND ri.goods_receipt_id=gr.id),'quantity_total',COALESCE((SELECT SUM(ri.quantity_received)::text FROM goods_receipt_items ri WHERE ri.company_id=gr.company_id AND ri.goods_receipt_id=gr.id),'0')) row FROM goods_receipts gr JOIN purchase_orders po ON po.id=gr.purchase_order_id AND po.company_id=gr.company_id JOIN suppliers sp ON sp.id=po.supplier_id AND sp.company_id=po.company_id WHERE gr.company_id=$1 AND gr.received_at >= $2 AND gr.received_at < $3 ORDER BY gr.received_at DESC,gr.id DESC LIMIT $4 OFFSET $5"#).bind(company).bind(from).bind(to).bind(limit).bind(offset).fetch_all(db).await?;
    Ok(
        json!({"items":rows.into_iter().map(|r|r.try_get::<Value,_>("row")).collect::<Result<Vec<_>,_>>()?,"limit":limit,"offset":offset}),
    )
}

pub async fn outstanding_purchase_orders(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (limit, offset) = paging(q);
    let rows=sqlx::query(r#"SELECT jsonb_build_object('id',po.id,'order_number',po.order_number,'supplier_id',po.supplier_id,'supplier_name',s.name,'status',po.status,'order_date',po.order_date,'total_amount',po.total_amount::text,'currency_code',(SELECT default_currency::text FROM companies WHERE id=$1),'open_quantity',COALESCE(SUM(GREATEST(i.ordered_quantity-i.received_quantity,0)),0)::text,'estimated_open_value',COALESCE(SUM(GREATEST(i.ordered_quantity-i.received_quantity,0)*i.unit_cost),0)::text) row FROM purchase_orders po JOIN suppliers s ON s.id=po.supplier_id AND s.company_id=po.company_id JOIN purchase_order_items i ON i.purchase_order_id=po.id AND i.company_id=po.company_id WHERE po.company_id=$1 AND po.status IN ('ordered','partially_received') GROUP BY po.id,s.name ORDER BY po.order_date,po.id LIMIT $2 OFFSET $3"#).bind(company).bind(limit).bind(offset).fetch_all(db).await?;
    Ok(
        json!({"items":rows.into_iter().map(|r|r.try_get::<Value,_>("row")).collect::<Result<Vec<_>,_>>()?,"limit":limit,"offset":offset}),
    )
}

pub async fn supplier_purchase_history(
    db: &PgPool,
    company: Uuid,
    supplier: i64,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let (limit, offset) = paging(q);
    if sqlx::query_scalar::<_, i64>("SELECT id FROM suppliers WHERE id=$1 AND company_id=$2")
        .bind(supplier)
        .bind(company)
        .fetch_optional(db)
        .await?
        .is_none()
    {
        return Err(ReportingError::NotFound);
    }
    let rows=sqlx::query(r#"SELECT jsonb_build_object('purchase_order_id',po.id,'order_number',po.order_number,'status',po.status,'order_date',po.order_date,'total_amount',po.total_amount::text,'purchase_order_currency_code',(SELECT default_currency::text FROM companies WHERE id=po.company_id),'received_quantity',COALESCE((SELECT SUM(ri.quantity_received)::text FROM goods_receipts gr JOIN goods_receipt_items ri ON ri.goods_receipt_id=gr.id AND ri.company_id=gr.company_id WHERE gr.purchase_order_id=po.id AND gr.company_id=po.company_id),'0'),'posted_invoices',COALESCE((SELECT jsonb_agg(jsonb_build_object('id',i.id,'invoice_number',i.invoice_number,'invoice_date',i.invoice_date,'currency_code',btrim(i.currency_code),'total_amount',i.total_amount::text) ORDER BY i.invoice_date,i.id) FROM supplier_invoices i WHERE i.company_id=po.company_id AND i.supplier_id=po.supplier_id AND i.purchase_order_id=po.id AND i.status='posted'),'[]'::jsonb)) row FROM purchase_orders po WHERE po.company_id=$1 AND po.supplier_id=$2 AND po.order_date >= ($3 AT TIME ZONE 'UTC')::date AND po.order_date < ($4 AT TIME ZONE 'UTC')::date ORDER BY po.order_date DESC,po.id DESC LIMIT $5 OFFSET $6"#).bind(company).bind(supplier).bind(from).bind(to).bind(limit).bind(offset).fetch_all(db).await?;
    Ok(
        json!({"items":rows.into_iter().map(|r|r.try_get::<Value,_>("row")).collect::<Result<Vec<_>,_>>()?,"limit":limit,"offset":offset}),
    )
}

pub async fn cash_registers(db: &PgPool, company: Uuid) -> Result<Value, ReportingError> {
    value(db,r#"SELECT COALESCE(jsonb_agg(jsonb_build_object('id',r.id,'code',r.code,'name',r.name,'location_id',r.location_id,'is_active',r.is_active,'open_session_id',s.id,'session_status',s.status,'opened_at',s.opened_at) ORDER BY r.id),'[]'::jsonb) FROM cash_registers r LEFT JOIN LATERAL(SELECT id,status,opened_at FROM cash_register_sessions WHERE company_id=r.company_id AND register_id=r.id AND status='open' ORDER BY opened_at DESC LIMIT 1)s ON true WHERE r.company_id=$1"#,company).await
}

pub async fn cash_sessions(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let (limit, offset) = paging(q);
    let rows=sqlx::query(r#"WITH events AS (SELECT s.id,s.company_id,s.opening_float,s.status,s.opened_at,s.closed_at,s.expected_cash,s.closing_cash_count,s.cash_variance,r.code register_code,r.name register_name,COALESCE((SELECT SUM(p.amount) FROM sale_payments p WHERE p.company_id=s.company_id AND p.cash_session_id=s.id AND p.payment_method='cash'),0) cash_sales,COALESCE((SELECT SUM(f.amount) FROM sale_refunds f WHERE f.company_id=s.company_id AND f.cash_session_id=s.id AND f.payment_method='cash'),0) cash_refunds,COALESCE((SELECT SUM(p.amount) FROM customer_payments p WHERE p.company_id=s.company_id AND p.cash_session_id=s.id AND p.payment_method='cash'),0) customer_receipts,COALESCE((SELECT SUM(p.amount) FROM supplier_payments p WHERE p.company_id=s.company_id AND p.cash_session_id=s.id AND p.payment_method='cash'),0) supplier_payments,COALESCE((SELECT SUM(m.amount) FROM cash_movements m WHERE m.company_id=s.company_id AND m.register_session_id=s.id AND m.movement_type='cash_in'),0) cash_in,COALESCE((SELECT SUM(m.amount) FROM cash_movements m WHERE m.company_id=s.company_id AND m.register_session_id=s.id AND m.movement_type IN('cash_out','paid_out','cash_drop')),0) cash_out FROM cash_register_sessions s JOIN cash_registers r ON r.id=s.register_id AND r.company_id=s.company_id WHERE s.company_id=$1 AND s.opened_at >= $2 AND s.opened_at < $3) SELECT jsonb_build_object('id',id,'register_code',register_code,'register_name',register_name,'status',status,'opened_at',opened_at,'closed_at',closed_at,'currency_code',(SELECT default_currency::text FROM companies WHERE id=company_id),'opening_float',opening_float::text,'cash_sales',cash_sales::text,'cash_refunds',cash_refunds::text,'customer_receipts',customer_receipts::text,'supplier_payments',supplier_payments::text,'cash_in',cash_in::text,'cash_out',cash_out::text,'calculated_expected_cash',(opening_float+cash_sales-cash_refunds+customer_receipts-supplier_payments+cash_in-cash_out)::text,'stored_expected_cash',expected_cash::text,'closing_cash_count',closing_cash_count::text,'cash_variance',cash_variance::text) row FROM events ORDER BY opened_at DESC,id DESC LIMIT $4 OFFSET $5"#).bind(company).bind(from).bind(to).bind(limit).bind(offset).fetch_all(db).await?;
    Ok(
        json!({"items":rows.into_iter().map(|r|r.try_get::<Value,_>("row")).collect::<Result<Vec<_>,_>>()?,"limit":limit,"offset":offset}),
    )
}

pub async fn cash_movements(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let (limit, offset) = paging(q);
    let rows=sqlx::query(r#"SELECT jsonb_build_object('id',m.id,'session_id',m.register_session_id,'register_id',s.register_id,'movement_type',m.movement_type,'currency_code',(SELECT default_currency::text FROM companies WHERE id=m.company_id),'amount',m.amount::text,'reference',m.reference,'created_at',m.created_at,'created_by',m.created_by) row FROM cash_movements m JOIN cash_register_sessions s ON s.id=m.register_session_id AND s.company_id=m.company_id WHERE m.company_id=$1 AND m.created_at >= $2 AND m.created_at < $3 ORDER BY m.created_at DESC,m.id DESC LIMIT $4 OFFSET $5"#).bind(company).bind(from).bind(to).bind(limit).bind(offset).fetch_all(db).await?;
    Ok(
        json!({"items":rows.into_iter().map(|r|r.try_get::<Value,_>("row")).collect::<Result<Vec<_>,_>>()?,"limit":limit,"offset":offset}),
    )
}

#[cfg(test)]
mod tests {
    use super::{ReportingError, bounds, paging, threshold};
    use crate::models::reporting::ReportQuery;
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[test]
    fn date_range_defaults_to_one_day_and_rejects_reversed_ranges() {
        let default = ReportQuery::default();
        let (from, to) = bounds(&default).unwrap();
        assert_eq!((to - from).num_days(), 1);

        let invalid = ReportQuery {
            from: Some(NaiveDate::from_ymd_opt(2026, 2, 2).unwrap()),
            to: Some(NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
            ..ReportQuery::default()
        };
        assert!(matches!(bounds(&invalid), Err(ReportingError::Invalid(_))));
    }

    #[test]
    fn pagination_and_explicit_low_stock_threshold_are_bounded() {
        let query = ReportQuery {
            limit: Some(500),
            offset: Some(-2),
            threshold: Some("3.2500".into()),
            ..ReportQuery::default()
        };
        assert_eq!(paging(&query), (100, 0));
        assert_eq!(
            threshold(&query).unwrap(),
            Some(Decimal::from_str("3.2500").unwrap())
        );
        assert!(matches!(threshold(&ReportQuery::default()), Ok(None)));
    }
}

/// Gross profit for completed sales in the range: revenue (net of discounts, excluding tax)
/// minus cost of goods sold, using the cost snapshot taken when the sale line was created
/// (falling back to the variant's current cost for older lines). Returns are not deducted.
pub async fn gross_profit(
    db: &PgPool,
    company: Uuid,
    q: &ReportQuery,
) -> Result<Value, ReportingError> {
    let (from, to) = bounds(q)?;
    let (limit, offset) = paging(q);
    let sql = r#"WITH items AS (
        SELECT si.variant_id, si.product_name, si.variant_name, si.quantity,
               (si.line_subtotal - si.discount_amount) AS revenue,
               COALESCE(si.unit_cost, v.cost_price) AS unit_cost
        FROM sales s
        JOIN sale_items si ON si.sale_id = s.id AND si.company_id = s.company_id
        JOIN product_variants v ON v.id = si.variant_id AND v.company_id = si.company_id
        WHERE s.company_id = $1 AND s.status = 'completed'
          AND s.completed_at >= $2 AND s.completed_at < $3
    ), per_product AS (
        SELECT variant_id, product_name, variant_name,
               SUM(quantity) qty, SUM(revenue) revenue,
               SUM(quantity * COALESCE(unit_cost,0)) cogs,
               COUNT(*) FILTER (WHERE unit_cost IS NULL) missing_cost
        FROM items GROUP BY variant_id, product_name, variant_name
    ), page AS (
        SELECT * FROM per_product ORDER BY (revenue - cogs) DESC, variant_id LIMIT $4 OFFSET $5
    )
    SELECT jsonb_build_object(
        'currency_code', (SELECT default_currency::text FROM companies WHERE id=$1),
        'from', ($2 AT TIME ZONE 'UTC')::date::text,
        'to', (($3 AT TIME ZONE 'UTC')::date - 1)::text,
        'basis', 'completed sales, net of discounts, excluding tax, before returns',
        'revenue', COALESCE((SELECT SUM(revenue) FROM items),0)::text,
        'cost_of_goods_sold', COALESCE((SELECT SUM(quantity * COALESCE(unit_cost,0)) FROM items),0)::text,
        'gross_profit', COALESCE((SELECT SUM(revenue - quantity * COALESCE(unit_cost,0)) FROM items),0)::text,
        'margin_percent', (SELECT CASE WHEN COALESCE(SUM(revenue),0) = 0 THEN NULL
            ELSE ROUND(SUM(revenue - quantity * COALESCE(unit_cost,0)) / SUM(revenue) * 100, 2)::text END FROM items),
        'lines_without_cost', (SELECT COUNT(*) FROM items WHERE unit_cost IS NULL),
        'by_product', COALESCE((SELECT jsonb_agg(jsonb_build_object(
            'variant_id', variant_id, 'product_name', product_name, 'variant_name', variant_name,
            'quantity_sold', qty::text, 'revenue', revenue::text, 'cost_of_goods_sold', cogs::text,
            'gross_profit', (revenue - cogs)::text, 'lines_without_cost', missing_cost
        ) ORDER BY (revenue - cogs) DESC, variant_id) FROM page), '[]'::jsonb),
        'limit', $4, 'offset', $5
    )"#;
    Ok(sqlx::query_scalar::<_, Value>(sql)
        .bind(company)
        .bind(from)
        .bind(to)
        .bind(limit)
        .bind(offset)
        .fetch_one(db)
        .await?)
}
