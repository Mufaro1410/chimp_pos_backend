-- Role configuration and system role support

ALTER TABLE roles
ADD COLUMN is_system_role BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE roles
ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE;

-- Stable identifier for built-in roles.
-- Custom roles can leave this NULL.
ALTER TABLE roles
ADD COLUMN role_key VARCHAR(50);

-- Prevent empty or whitespace-only role names.
ALTER TABLE roles
ADD CONSTRAINT roles_name_not_empty
CHECK (LENGTH(TRIM(name)) > 0);

-- Role keys must use lowercase snake_case.
ALTER TABLE roles
ADD CONSTRAINT roles_role_key_format
CHECK (
    role_key IS NULL
    OR role_key ~ '^[a-z][a-z0-9_]*$'
);

-- Every system role must have a stable key.
ALTER TABLE roles
ADD CONSTRAINT system_roles_require_key
CHECK (
    is_system_role = FALSE
    OR role_key IS NOT NULL
);

-- A company cannot have duplicate role keys.
CREATE UNIQUE INDEX idx_roles_company_role_key
ON roles(company_id, role_key)
WHERE role_key IS NOT NULL;

-- Efficiently retrieve active roles belonging to a company.
CREATE INDEX idx_roles_company_active
ON roles(company_id, is_active);