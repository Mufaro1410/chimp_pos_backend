-- Add migration script here

CREATE TABLE companies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Business identity
    name VARCHAR(150) NOT NULL,
    business_type VARCHAR(80),
    description TEXT,

    -- Contact information
    email VARCHAR(255),
    phone VARCHAR(30),
    address TEXT,
    city VARCHAR(100),

    -- Regional configuration
    country_code CHAR(2) NOT NULL DEFAULT 'ZW',
    timezone VARCHAR(64) NOT NULL DEFAULT 'Africa/Harare',
    locale VARCHAR(35) NOT NULL DEFAULT 'en-ZW',
    default_currency CHAR(3) NOT NULL DEFAULT 'USD',

    -- Account status
    status VARCHAR(20) NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'suspended', 'archived')),

    -- Record management
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INTEGER NOT NULL DEFAULT 1
        CHECK (version > 0)
);

CREATE INDEX idx_companies_status
    ON companies(status);

CREATE INDEX idx_companies_country
    ON companies(country_code);


CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    company_id UUID NOT NULL
        REFERENCES companies(id) ON DELETE RESTRICT,

    -- Identity
    name VARCHAR(150) NOT NULL,
    email VARCHAR(255),
    username VARCHAR(100) NOT NULL,

    -- Authentication
    password_hash TEXT NOT NULL,

    -- Authorisation
    role VARCHAR(30) NOT NULL DEFAULT 'cashier'
        CHECK (
            role IN ('owner', 'admin', 'manager', 'cashier')
        ),

    status VARCHAR(20) NOT NULL DEFAULT 'active'
        CHECK (
            status IN ('active', 'inactive', 'suspended')
        ),

    -- Account activity
    last_login_at TIMESTAMPTZ,

    -- Record management
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INTEGER NOT NULL DEFAULT 1
        CHECK (version > 0)
);

CREATE UNIQUE INDEX idx_users_company_username
    ON users(company_id, LOWER(username));

CREATE UNIQUE INDEX idx_users_company_email
    ON users(company_id, LOWER(email))
    WHERE email IS NOT NULL;

CREATE INDEX idx_users_company_status
    ON users(company_id, status);
