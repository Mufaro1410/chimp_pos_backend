-- Add migration script here

-- Add purchasing permissions.

INSERT INTO permissions (
    permission_key,
    resource,
    action,
    description
)
VALUES
    ('purchases.update', 'purchases', 'update',
     'Update draft purchase orders'),

    ('purchases.receive', 'purchases', 'receive',
     'Receive goods against purchase orders'),

    ('purchases.cancel', 'purchases', 'cancel',
     'Cancel eligible purchase orders')

ON CONFLICT (permission_key) DO NOTHING;


-- Owner and Admin receive all purchasing permissions.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.is_system_role = TRUE
  AND r.role_key IN ('owner', 'admin')
  AND p.permission_key IN (
      'purchases.view',
      'purchases.create',
      'purchases.update',
      'purchases.receive',
      'purchases.cancel'
  )
ON CONFLICT (role_id, permission_id) DO NOTHING;


-- Manager can view, create, update and receive purchases.
-- Cancellation remains restricted to Owner and Admin.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
CROSS JOIN permissions p
WHERE r.is_system_role = TRUE
  AND r.role_key = 'manager'
  AND p.permission_key IN (
      'purchases.view',
      'purchases.create',
      'purchases.update',
      'purchases.receive'
  )
ON CONFLICT (role_id, permission_id) DO NOTHING;
