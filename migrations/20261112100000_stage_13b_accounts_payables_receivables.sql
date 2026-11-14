-- Stage 13B: customer receivables and supplier payables.
-- All monetary values use NUMERIC(19,4). Posted financial records are immutable
-- by application policy; corrections are represented by credit notes/reversals.

-- Number allocation is transaction-safe and company scoped.
INSERT INTO customer_sequences(company_id, next_number)
SELECT id, 1 FROM companies ON CONFLICT(company_id) DO NOTHING;
INSERT INTO supplier_sequences(company_id, next_number)
SELECT c.id, COALESCE(MAX(s.supplier_number),0)+1
FROM companies c LEFT JOIN suppliers s ON s.company_id=c.id
GROUP BY c.id
ON CONFLICT(company_id) DO UPDATE SET next_number=GREATEST(supplier_sequences.next_number, EXCLUDED.next_number);

-- Customer receipts against an account. Checkout payments remain in sale_payments.
CREATE TABLE customer_payments (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  company_id UUID NOT NULL,
  customer_id BIGINT NOT NULL,
  payment_number BIGINT NOT NULL,
  amount NUMERIC(19,4) NOT NULL CHECK(amount > 0),
  payment_method VARCHAR(30) NOT NULL CHECK(payment_method IN ('cash','card','mobile_money','other')),
  reference VARCHAR(150), notes TEXT,
  cash_session_id BIGINT,
  idempotency_key UUID NOT NULL,
  request_fingerprint TEXT NOT NULL CHECK(length(trim(request_fingerprint)) > 0),
  received_by UUID NOT NULL,
  received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT fk_customer_payment_customer FOREIGN KEY(customer_id,company_id) REFERENCES customers(id,company_id) ON DELETE RESTRICT,
  CONSTRAINT fk_customer_payment_user FOREIGN KEY(received_by,company_id) REFERENCES users(id,company_id) ON DELETE RESTRICT,
  CONSTRAINT fk_customer_payment_cash_session FOREIGN KEY(cash_session_id,company_id) REFERENCES cash_register_sessions(id,company_id) ON DELETE RESTRICT,
  CONSTRAINT uq_customer_payment_number UNIQUE(company_id,payment_number),
  CONSTRAINT uq_customer_payment_id_company UNIQUE(id,company_id),
  CONSTRAINT uq_customer_payment_idempotency UNIQUE(company_id,idempotency_key),
  CONSTRAINT chk_customer_payment_cash_session CHECK(payment_method='cash' OR cash_session_id IS NULL)
);
CREATE INDEX idx_customer_payments_customer_date ON customer_payments(company_id,customer_id,received_at DESC);
CREATE TABLE customer_payment_sequences(company_id UUID PRIMARY KEY REFERENCES companies(id) ON DELETE RESTRICT, next_number BIGINT NOT NULL CHECK(next_number>0));
INSERT INTO customer_payment_sequences(company_id,next_number) SELECT id,1 FROM companies ON CONFLICT DO NOTHING;

CREATE TABLE customer_payment_allocations (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  company_id UUID NOT NULL,
  payment_id BIGINT NOT NULL,
  sale_id BIGINT NOT NULL,
  amount NUMERIC(19,4) NOT NULL CHECK(amount>0),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY(payment_id,company_id) REFERENCES customer_payments(id,company_id) ON DELETE RESTRICT,
  FOREIGN KEY(sale_id,company_id) REFERENCES sales(id,company_id) ON DELETE RESTRICT,
  UNIQUE(payment_id,sale_id)
);
CREATE INDEX idx_customer_allocations_sale ON customer_payment_allocations(company_id,sale_id);

-- Supplier invoices are the event that creates a payable. A PO or goods receipt
-- alone does not create a liability or alter stock.
CREATE TABLE supplier_invoices (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  company_id UUID NOT NULL,
  supplier_id BIGINT NOT NULL,
  invoice_number VARCHAR(100) NOT NULL,
  supplier_reference VARCHAR(150),
  purchase_order_id BIGINT,
  invoice_date DATE NOT NULL DEFAULT CURRENT_DATE,
  due_date DATE,
  currency_code CHAR(3) NOT NULL DEFAULT 'USD',
  subtotal NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK(subtotal>=0),
  tax_total NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK(tax_total>=0),
  discount_total NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK(discount_total>=0),
  additional_charges NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK(additional_charges>=0),
  total_amount NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK(total_amount>=0),
  status VARCHAR(20) NOT NULL DEFAULT 'draft' CHECK(status IN ('draft','posted','void')),
  notes TEXT,
  idempotency_key UUID NOT NULL,
  request_fingerprint TEXT NOT NULL CHECK(length(trim(request_fingerprint))>0),
  created_by UUID NOT NULL,
  posted_by UUID,
  posted_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  version INTEGER NOT NULL DEFAULT 1 CHECK(version>0),
  FOREIGN KEY(supplier_id,company_id) REFERENCES suppliers(id,company_id) ON DELETE RESTRICT,
  FOREIGN KEY(purchase_order_id,company_id) REFERENCES purchase_orders(id,company_id) ON DELETE RESTRICT,
  FOREIGN KEY(created_by,company_id) REFERENCES users(id,company_id) ON DELETE RESTRICT,
  FOREIGN KEY(posted_by,company_id) REFERENCES users(id,company_id) ON DELETE RESTRICT,
  UNIQUE(id,company_id), UNIQUE(company_id,supplier_id,invoice_number), UNIQUE(company_id,idempotency_key),
  CHECK(due_date IS NULL OR due_date>=invoice_date),
  CHECK(total_amount = subtotal + tax_total - discount_total + additional_charges),
  CHECK((status='posted' AND posted_by IS NOT NULL AND posted_at IS NOT NULL) OR (status<>'posted' AND posted_by IS NULL AND posted_at IS NULL))
);
CREATE INDEX idx_supplier_invoices_supplier_date ON supplier_invoices(company_id,supplier_id,invoice_date DESC);
CREATE INDEX idx_supplier_invoices_due ON supplier_invoices(company_id,due_date) WHERE status='posted';

CREATE TABLE supplier_invoice_items (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  company_id UUID NOT NULL,
  invoice_id BIGINT NOT NULL,
  purchase_order_item_id BIGINT,
  goods_receipt_item_id BIGINT,
  description VARCHAR(255) NOT NULL CHECK(length(trim(description))>0),
  quantity NUMERIC(19,4) NOT NULL CHECK(quantity>0),
  unit_cost NUMERIC(19,4) NOT NULL CHECK(unit_cost>=0),
  tax_amount NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK(tax_amount>=0),
  discount_amount NUMERIC(19,4) NOT NULL DEFAULT 0 CHECK(discount_amount>=0),
  line_total NUMERIC(19,4) NOT NULL CHECK(line_total>=0),
  FOREIGN KEY(invoice_id,company_id) REFERENCES supplier_invoices(id,company_id) ON DELETE RESTRICT,
  FOREIGN KEY(purchase_order_item_id) REFERENCES purchase_order_items(id) ON DELETE RESTRICT,
  FOREIGN KEY(goods_receipt_item_id) REFERENCES goods_receipt_items(id) ON DELETE RESTRICT
);
CREATE INDEX idx_supplier_invoice_items_invoice ON supplier_invoice_items(company_id,invoice_id);

CREATE TABLE supplier_payment_sequences(company_id UUID PRIMARY KEY REFERENCES companies(id) ON DELETE RESTRICT, next_number BIGINT NOT NULL CHECK(next_number>0));
INSERT INTO supplier_payment_sequences(company_id,next_number) SELECT id,1 FROM companies ON CONFLICT DO NOTHING;
CREATE TABLE supplier_payments (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  company_id UUID NOT NULL,
  supplier_id BIGINT NOT NULL,
  payment_number BIGINT NOT NULL,
  amount NUMERIC(19,4) NOT NULL CHECK(amount>0),
  payment_method VARCHAR(30) NOT NULL CHECK(payment_method IN ('cash','card','mobile_money','other')),
  reference VARCHAR(150), notes TEXT,
  cash_session_id BIGINT,
  idempotency_key UUID NOT NULL,
  request_fingerprint TEXT NOT NULL CHECK(length(trim(request_fingerprint))>0),
  paid_by UUID NOT NULL,
  paid_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY(supplier_id,company_id) REFERENCES suppliers(id,company_id) ON DELETE RESTRICT,
  FOREIGN KEY(paid_by,company_id) REFERENCES users(id,company_id) ON DELETE RESTRICT,
  FOREIGN KEY(cash_session_id,company_id) REFERENCES cash_register_sessions(id,company_id) ON DELETE RESTRICT,
  UNIQUE(company_id,payment_number), UNIQUE(company_id,idempotency_key), UNIQUE(id,company_id),
  CHECK(payment_method='cash' OR cash_session_id IS NULL)
);
CREATE INDEX idx_supplier_payments_supplier_date ON supplier_payments(company_id,supplier_id,paid_at DESC);
CREATE TABLE supplier_payment_allocations (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  company_id UUID NOT NULL,
  payment_id BIGINT NOT NULL,
  invoice_id BIGINT NOT NULL,
  amount NUMERIC(19,4) NOT NULL CHECK(amount>0),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  FOREIGN KEY(payment_id,company_id) REFERENCES supplier_payments(id,company_id) ON DELETE RESTRICT,
  FOREIGN KEY(invoice_id,company_id) REFERENCES supplier_invoices(id,company_id) ON DELETE RESTRICT,
  UNIQUE(payment_id,invoice_id)
);
CREATE INDEX idx_supplier_allocations_invoice ON supplier_payment_allocations(company_id,invoice_id);

-- Credit notes reduce the relevant account balance; they never rewrite source records.
CREATE TABLE customer_credit_notes (
 id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY, company_id UUID NOT NULL, customer_id BIGINT NOT NULL,
 sale_id BIGINT, credit_number BIGINT NOT NULL, amount NUMERIC(19,4) NOT NULL CHECK(amount>0),
 reason TEXT NOT NULL CHECK(length(trim(reason))>0), idempotency_key UUID NOT NULL,
 created_by UUID NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
 FOREIGN KEY(customer_id,company_id) REFERENCES customers(id,company_id) ON DELETE RESTRICT,
 FOREIGN KEY(sale_id,company_id) REFERENCES sales(id,company_id) ON DELETE RESTRICT,
 FOREIGN KEY(created_by,company_id) REFERENCES users(id,company_id) ON DELETE RESTRICT,
 UNIQUE(company_id,credit_number), UNIQUE(company_id,idempotency_key)
);
CREATE TABLE supplier_credit_notes (
 id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY, company_id UUID NOT NULL, supplier_id BIGINT NOT NULL,
 invoice_id BIGINT NOT NULL, credit_number BIGINT NOT NULL, amount NUMERIC(19,4) NOT NULL CHECK(amount>0),
 reason TEXT NOT NULL CHECK(length(trim(reason))>0), idempotency_key UUID NOT NULL,
 created_by UUID NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
 FOREIGN KEY(supplier_id,company_id) REFERENCES suppliers(id,company_id) ON DELETE RESTRICT,
 FOREIGN KEY(invoice_id,company_id) REFERENCES supplier_invoices(id,company_id) ON DELETE RESTRICT,
 FOREIGN KEY(created_by,company_id) REFERENCES users(id,company_id) ON DELETE RESTRICT,
 UNIQUE(company_id,credit_number), UNIQUE(company_id,idempotency_key)
);

-- Read-only account summaries. Checkout payments and subsequent account receipts
-- are both credits against customer receivables. Refund cash is NOT double-counted
-- as an account credit; a return credit note represents the account adjustment.
CREATE VIEW customer_account_balances AS
SELECT c.company_id,c.id AS customer_id,
 COALESCE((SELECT SUM(s.total_amount) FROM sales s WHERE s.company_id=c.company_id AND s.customer_id=c.id AND s.status='completed'),0)
 - COALESCE((SELECT SUM(p.amount) FROM sale_payments p JOIN sales s ON s.id=p.sale_id AND s.company_id=p.company_id WHERE s.company_id=c.company_id AND s.customer_id=c.id AND s.status='completed'),0)
 - COALESCE((SELECT SUM(p.amount) FROM customer_payments p WHERE p.company_id=c.company_id AND p.customer_id=c.id),0)
 - COALESCE((SELECT SUM(n.amount) FROM customer_credit_notes n WHERE n.company_id=c.company_id AND n.customer_id=c.id),0) AS balance
FROM customers c;

CREATE VIEW supplier_account_balances AS
SELECT s.company_id,s.id AS supplier_id,
 COALESCE((SELECT SUM(i.total_amount) FROM supplier_invoices i WHERE i.company_id=s.company_id AND i.supplier_id=s.id AND i.status='posted'),0)
 - COALESCE((SELECT SUM(p.amount) FROM supplier_payments p WHERE p.company_id=s.company_id AND p.supplier_id=s.id),0)
 - COALESCE((SELECT SUM(n.amount) FROM supplier_credit_notes n WHERE n.company_id=s.company_id AND n.supplier_id=s.id),0) AS balance
FROM suppliers s;

-- Fine-grained permissions. Owner/admin are granted by role policy below.
INSERT INTO permissions(permission_key,resource,action,description) VALUES
 ('customers.view','customers','view','View customers and customer accounts'),
 ('customers.manage','customers','manage','Create and manage customers'),
 ('customers.credit','customers','credit','Authorize and manage customer credit'),
 ('customers.payment','customers','payment','Receive customer account payments'),
 ('suppliers.view','suppliers','view','View suppliers and supplier accounts'),
 ('suppliers.manage','suppliers','manage','Create and manage suppliers'),
 ('suppliers.invoice','suppliers','invoice','Create and post supplier invoices'),
 ('suppliers.payment','suppliers','payment','Record supplier account payments')
ON CONFLICT(permission_key) DO NOTHING;
INSERT INTO role_permissions(role_id,permission_id)
SELECT r.id,p.id FROM roles r CROSS JOIN permissions p
WHERE r.role_key IN ('owner','admin','manager')
AND p.permission_key IN ('customers.view','customers.manage','customers.credit','customers.payment','suppliers.view','suppliers.manage','suppliers.invoice','suppliers.payment')
ON CONFLICT(role_id,permission_id) DO NOTHING;
INSERT INTO role_permissions(role_id,permission_id)
SELECT r.id,p.id FROM roles r CROSS JOIN permissions p
WHERE r.role_key='cashier' AND p.permission_key IN ('customers.view','customers.payment','suppliers.view')
ON CONFLICT(role_id,permission_id) DO NOTHING;
