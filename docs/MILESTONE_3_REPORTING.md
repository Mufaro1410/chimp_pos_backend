# Milestone 3 — Reporting API

## Implemented endpoints

All reporting routes require authentication and the existing `reports.view` permission. Company scope comes only from the authenticated user context. Monetary and quantity values are serialized as decimal strings.

| Endpoint | Purpose |
|---|---|
| `GET /api/v1/reports/dashboard` | Today's sales totals/count/average, checkout payment-method totals, products, low/out-of-stock counts, recent completed sales and stock movements, registers and open-session status |
| `GET /api/v1/reports/sales/summary` | Gross/net sales, returns, refund payments, transaction count/average, day/week/month sales buckets |
| `GET /api/v1/reports/sales/by-product` | Sales and returned quantities/amounts by sale-line variant |
| `GET /api/v1/reports/sales/by-category` | Sales by the product's current category |
| `GET /api/v1/reports/sales/by-payment-method` | Checkout payment totals by method |
| `GET /api/v1/reports/sales/transactions` | Paged sale history with date, status, location and payment-method filters |
| `GET /api/v1/reports/sales/returns` | Return, refund and void event counts and amounts |
| `GET /api/v1/reports/sales/gross-profit` | Returns `501 HISTORICAL_COST_UNAVAILABLE`; no profit figure is fabricated |
| `GET /api/v1/reports/inventory/stock` | Current stock by product variant, optionally filtered to low/out/all and location |
| `GET /api/v1/reports/inventory/valuation` | Current on-hand valuation by current variant cost price, with uncosted stock reported separately |
| `GET /api/v1/reports/inventory/movements` | Paged stock movement history with date/location/variant filters |
| `GET /api/v1/reports/inventory/activity` | Received/sold stock movement quantity totals by date/location |
| `GET /api/v1/reports/purchasing/summary` | Purchase orders, goods receipts and posted supplier invoices as separate totals |
| `GET /api/v1/reports/purchasing/by-supplier` | Paged supplier purchasing, receipt and posted-invoice totals |
| `GET /api/v1/reports/purchasing/goods-received` | Paged receipts linked to purchase orders and suppliers |
| `GET /api/v1/reports/purchasing/outstanding-orders` | Ordered/partially received order quantities and estimated remaining line value |
| `GET /api/v1/reports/suppliers/{id}/purchase-history` | One supplier's purchase orders, receipts and posted invoices |
| `GET /api/v1/reports/cash/registers` | Register state and open session |
| `GET /api/v1/reports/cash/sessions` | Paged session opening/closing figures and calculated cash totals |
| `GET /api/v1/reports/cash/movements` | Paged manual cash movements |

Request parameters and response schemas are documented in [`openapi.yaml`](openapi.yaml). Range endpoints accept inclusive `from` and `to` UTC dates, default to today, and reject reversed ranges or ranges longer than ten years. List endpoints clamp `limit` to 1–100 and `offset` to nonnegative values.

## Report definitions

### Sales

- Gross sales are completed sale totals, tax inclusive, by `completed_at`. Draft and cancelled sales are excluded. A sale recorded in `sale_voids` is cancelled by the existing void workflow and is therefore excluded from gross sales; the void report separately counts its source sale amount.
- Returns reduce net sales by the `sale_return_items.refundable_amount` recorded in the return period. Refund totals report actual `sale_refunds.amount` paid in the refund period; they are shown separately and are not subtracted a second time from net sales.
- Sales payment-method totals use checkout `sale_payments`, by payment creation date. Account receipts and supplier payments are represented in cash-session totals, not checkout sales.
- Sales totals are labelled with the company's configured default currency because the applied sales schema stores no currency on sales or sale payments. This label does not resolve the separately documented historical sale-currency uncertainty.
- Product sales use the historical sale-item name/variant snapshots. Category reporting joins the current product category because category is not snapshotted on sale lines. Historical category changes can therefore change category attribution.
- Sale transaction history uses sale creation date and includes status, void marker, all-time refunds and checkout payment details.

### Inventory and valuation

- Current on-hand and reserved quantities come from `stock_balances`; available quantity is on-hand minus reserved.
- There is no configured reorder threshold in the schema. Low-stock results require a caller-supplied nonnegative `threshold`; the dashboard low-stock count is `null` unless this threshold is supplied. Out-of-stock products are counted separately where all active variants have no on-hand stock.
- Received and sold quantities use `stock_movements` with types `purchase_receipt` and `sale` respectively. Labels use current product/variant names because stock movement rows do not retain name snapshots.
- Current inventory valuation is on-hand quantity multiplied by the current `product_variants.cost_price`. Variants without a cost are excluded from the valued total and reported as uncosted quantity/count. This is a current estimate, not a historical valuation.
- Historical gross profit is unavailable. Sale items retain sale price/discount/tax but not unit cost at sale time; current variant costs cannot safely reconstruct historical COGS. The endpoint explicitly returns 501.

### Purchasing

- Purchase-order value is reported as order value, not a payable. Goods receipts are physical received quantities/events and do not create a payable. Only posted supplier invoices are counted as payables.
- Supplier invoice amounts are grouped by their stored currency and are not summed across currencies. Purchase-order totals follow the shop's configured base-currency convention because purchase orders do not store a currency.
- Outstanding order value is an estimate based on remaining ordered quantity multiplied by the purchase-order line's stored unit cost. It is not an invoice balance.
- Supplier report aggregates are calculated independently before joining, avoiding row multiplication between order, receipt and invoice tables.

### Cash

- Session calculated cash is opening float + cash checkout receipts − cash refunds + cash customer receipts − cash supplier payments + manual cash-in − manual cash-out/paid-out/drops.
- Source payment/refund tables are read directly. Reporting does not insert cash movements, so events already included in cash-session calculation are not duplicated.
- Pending reversal-event tables are not assumed or queried. Reversal amounts remain outside reports until their migration is separately approved and applied.

## Permissions, scope and performance

- The pre-existing `reports.view` permission is used; no new permission migration or RBAC model was added.
- Every query is filtered by the authenticated `company_id`; supplier history additionally verifies that the supplier belongs to that company.
- Reports use SQL aggregation and bounded pagination. The supplier purchasing report aggregates purchase orders, receipts and invoices in separate CTEs to prevent join multiplication. Dashboard recent lists are capped at ten rows.
- Range timestamps are bound as UTC timestamps. Dashboard “today” uses each company's configured timezone. Report amounts are cast to text in JSON to preserve decimal precision.
- No report query has been exercised against PostgreSQL in this milestone. SQL syntax, query plans, and behavior against the applied schema still need the dedicated integration verification phase.

## Tests and results

- Added unit tests for date-range validation/defaults, pagination bounds, and explicit low-stock threshold parsing.
- Added permission mapping checks for dashboard, sales, inventory, purchasing and cash reports. The router authentication test now includes representative report paths.
- PostgreSQL integration tests were not added or run; the dedicated PostgreSQL verification phase remains outstanding by design. The existing opt-in financial test harness is unrelated to report query execution.

## Known limitations and outstanding work

1. PostgreSQL integration tests must execute the report queries against a disposable database created from the applied base migrations. Do not apply pending financial-hardening or reversal migrations as part of report validation.
2. Report query plans need review against representative data volumes; most ranges have useful company/date indexes, but large aggregate scans may require further indexes after measurement.
3. Low-stock values depend on a request-supplied threshold because no company/product reorder level exists.
4. Historical gross profit, historical inventory valuation, and historical product-category attribution cannot be reconstructed reliably from current schema data.
5. Returns and refunds are separate events; the report intentionally exposes both. Accounting owners should confirm this operational definition before using these figures for statutory reporting.
6. Pending reversal events are not included in report totals until reversal schema deployment is separately approved.
