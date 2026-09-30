-- Add the independently authorized receipt recovery capability without
-- changing existing financial or print history.
INSERT INTO permissions(code,description)
VALUES('receipt.reprint','Requeue failed receipt printing')
ON CONFLICT(code) DO UPDATE SET description=excluded.description;

INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT id,'receipt.reprint'
FROM roles
WHERE lower(name) IN ('owner','administrator','admin');
