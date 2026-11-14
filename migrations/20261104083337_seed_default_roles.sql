-- Add migration script here
-- Create default roles for every existing company.

INSERT INTO roles (
    company_id,
    name,
    description,
    is_system_role,
    is_active,
    role_key
)
SELECT
    c.id,
    r.name,
    r.description,
    TRUE,
    TRUE,
    r.role_key
FROM companies c
CROSS JOIN (
    VALUES
        ('Owner', 'Full access to all business operations.', 'owner'),
        ('Admin', 'Business and staff administration.', 'admin'),
        ('Manager', 'Day-to-day business operations.', 'manager'),
        ('Cashier', 'Sales and customer service operations.', 'cashier')
) AS r(name, description, role_key)
ON CONFLICT (company_id, role_key)
WHERE role_key IS NOT NULL
DO NOTHING;

-- Owner receives every permission.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.role_key = 'owner'
  AND r.is_system_role = TRUE
ON CONFLICT (role_id, permission_id) DO NOTHING;


-- Admin receives all permissions except role and settings management.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.role_key = 'admin'
  AND r.is_system_role = TRUE
  AND p.permission_key NOT IN (
      'roles.manage',
      'settings.manage'
  )
ON CONFLICT (role_id, permission_id) DO NOTHING;


-- Manager permissions.
WITH manager_permissions(permission_key) AS (
    VALUES
        ('dashboard.view'),
        ('categories.view'),
        ('categories.manage'),
        ('customers.view'),
        ('customers.manage'),
        ('products.view'),
        ('products.create'),
        ('products.update'),
        ('inventory.view'),
        ('inventory.adjust'),
        ('inventory.receive'),
        ('inventory.transfer'),
        ('sales.view'),
        ('sales.create'),
        ('sales.discount'),
        ('sales.refund'),
        ('sales.void'),
        ('suppliers.view'),
        ('suppliers.manage'),
        ('purchases.view'),
        ('purchases.create'),
        ('expenses.view'),
        ('expenses.manage'),
        ('reports.view'),
        ('reports.export'),
        ('users.view')
)
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN manager_permissions mp ON TRUE
JOIN permissions p ON p.permission_key = mp.permission_key
WHERE r.role_key = 'manager'
  AND r.is_system_role = TRUE
ON CONFLICT (role_id, permission_id) DO NOTHING;


-- Cashier permissions.
WITH cashier_permissions(permission_key) AS (
    VALUES
        ('dashboard.view'),
        ('products.view'),
        ('inventory.view'),
        ('customers.view'),
        ('customers.manage'),
        ('sales.view'),
        ('sales.create')
)
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN cashier_permissions cp ON TRUE
JOIN permissions p ON p.permission_key = cp.permission_key
WHERE r.role_key = 'cashier'
  AND r.is_system_role = TRUE
ON CONFLICT (role_id, permission_id) DO NOTHING;
