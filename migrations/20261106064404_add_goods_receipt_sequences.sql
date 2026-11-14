-- Add migration script here
CREATE TABLE goods_receipt_sequences (
    company_id UUID PRIMARY KEY,
    next_number BIGINT NOT NULL DEFAULT 1,

    CONSTRAINT fk_goods_receipt_sequences_company
        FOREIGN KEY (company_id)
        REFERENCES companies(id)
        ON DELETE RESTRICT,

    CONSTRAINT chk_goods_receipt_sequences_next_number
        CHECK (next_number > 0)
);