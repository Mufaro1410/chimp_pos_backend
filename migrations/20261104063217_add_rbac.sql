-- Add migration script here
CREATE TABLE permissions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    permission_key VARCHAR(100) NOT NULL UNIQUE,
    resource VARCHAR(50) NOT NULL,
    action VARCHAR(50) NOT NULL,
    description TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT permissions_resource_action_unique
        UNIQUE (resource, action)
);

INSERT INTO permissions (permission_key, resource, action, description)
VALUES
    ('dashboard.view', 'dashboard', 'view', 'View business dashboard'),

    ('products.view', 'products', 'view', 'View products'),
    ('products.create', 'products', 'create', 'Create products'),
    ('products.update', 'products', 'update', 'Update products'),
    ('products.archive', 'products', 'archive', 'Archive products'),

    ('categories.view', 'categories', 'view', 'View categories'),
    ('categories.manage', 'categories', 'manage', 'Manage categories'),

    ('inventory.view', 'inventory', 'view', 'View inventory'),
    ('inventory.adjust', 'inventory', 'adjust', 'Adjust stock'),
    ('inventory.receive', 'inventory', 'receive', 'Receive stock'),
    ('inventory.transfer', 'inventory', 'transfer', 'Transfer stock'),

    ('sales.view', 'sales', 'view', 'View sales'),
    ('sales.create', 'sales', 'create', 'Create sales'),
    ('sales.void', 'sales', 'void', 'Void sales'),
    ('sales.refund', 'sales', 'refund', 'Process refunds'),
    ('sales.discount', 'sales', 'discount', 'Apply discounts'),

    ('customers.view', 'customers', 'view', 'View customers'),
    ('customers.manage', 'customers', 'manage', 'Manage customers'),

    ('suppliers.view', 'suppliers', 'view', 'View suppliers'),
    ('suppliers.manage', 'suppliers', 'manage', 'Manage suppliers'),

    ('purchases.view', 'purchases', 'view', 'View purchases'),
    ('purchases.create', 'purchases', 'create', 'Create purchases'),

    ('expenses.view', 'expenses', 'view', 'View expenses'),
    ('expenses.manage', 'expenses', 'manage', 'Manage expenses'),

    ('reports.view', 'reports', 'view', 'View reports'),
    ('reports.export', 'reports', 'export', 'Export reports'),

    ('users.view', 'users', 'view', 'View users'),
    ('users.manage', 'users', 'manage', 'Manage users'),

    ('roles.view', 'roles', 'view', 'View roles'),
    ('roles.manage', 'roles', 'manage', 'Manage roles'),

    ('settings.view', 'settings', 'view', 'View settings'),
    ('settings.manage', 'settings', 'manage', 'Manage settings'),

    ('audit_logs.view', 'audit_logs', 'view', 'View audit logs');


CREATE TABLE roles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    company_id UUID NOT NULL
        REFERENCES companies(id) ON DELETE RESTRICT,

    name VARCHAR(100) NOT NULL,
    description TEXT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),

    CONSTRAINT roles_id_company_unique
        UNIQUE (id, company_id)
);

CREATE UNIQUE INDEX idx_roles_company_name
    ON roles(company_id, LOWER(name));

CREATE TABLE role_permissions (
    role_id UUID NOT NULL
        REFERENCES roles(id) ON DELETE CASCADE,

    permission_id UUID NOT NULL
        REFERENCES permissions(id) ON DELETE RESTRICT,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    PRIMARY KEY (role_id, permission_id)
);

CREATE INDEX idx_role_permissions_permission
    ON role_permissions(permission_id);

ALTER TABLE users
    ADD CONSTRAINT users_id_company_unique
    UNIQUE (id, company_id);

CREATE TABLE user_roles (
    company_id UUID NOT NULL,

    user_id UUID NOT NULL,
    role_id UUID NOT NULL,

    assigned_by UUID,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,

    PRIMARY KEY (user_id, role_id),

    CONSTRAINT user_roles_user_company_fk
        FOREIGN KEY (user_id, company_id)
        REFERENCES users(id, company_id)
        ON DELETE CASCADE,

    CONSTRAINT user_roles_role_company_fk
        FOREIGN KEY (role_id, company_id)
        REFERENCES roles(id, company_id)
        ON DELETE CASCADE,

    CONSTRAINT user_roles_assigned_by_company_fk
        FOREIGN KEY (assigned_by, company_id)
        REFERENCES users(id, company_id)
        ON DELETE SET NULL (assigned_by),

    CONSTRAINT user_roles_expiry_check
        CHECK (expires_at IS NULL OR expires_at > assigned_at)
);

CREATE INDEX idx_user_roles_company
    ON user_roles(company_id);

CREATE INDEX idx_user_roles_role
    ON user_roles(role_id);

ALTER TABLE users DROP COLUMN role;