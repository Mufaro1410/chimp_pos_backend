-- Stage 12 hardening: grant cash-register permissions using stable system role keys.
-- This complements the original Stage 12 migration and repairs existing companies.

WITH role_permission_grants(role_key, permission_key) AS (
    VALUES
        ('owner', 'cash_register.view'),
        ('owner', 'cash_register.manage'),
        ('owner', 'cash_register.open'),
        ('owner', 'cash_register.close'),
        ('owner', 'cash_register.movement'),
        ('admin', 'cash_register.view'),
        ('admin', 'cash_register.manage'),
        ('admin', 'cash_register.open'),
        ('admin', 'cash_register.close'),
        ('admin', 'cash_register.movement'),
        ('manager', 'cash_register.view'),
        ('manager', 'cash_register.open'),
        ('manager', 'cash_register.close'),
        ('manager', 'cash_register.movement'),
        ('cashier', 'cash_register.view'),
        ('cashier', 'cash_register.open'),
        ('cashier', 'cash_register.close'),
        ('cashier', 'cash_register.movement'),
        ('manager', 'sales.checkout'),
        ('manager', 'sales.return'),
        ('cashier', 'sales.checkout')
)
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN role_permission_grants g ON g.role_key = r.role_key
JOIN permissions p ON p.permission_key = g.permission_key
WHERE r.is_system_role = TRUE
  AND r.is_active = TRUE
ON CONFLICT (role_id, permission_id) DO NOTHING;
