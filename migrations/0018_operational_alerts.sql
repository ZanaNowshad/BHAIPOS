CREATE TABLE IF NOT EXISTS alert_operation_results (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  operation_id TEXT NOT NULL,
  action TEXT NOT NULL,
  request_sha256 TEXT NOT NULL,
  result_json TEXT NOT NULL,
  committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id),
  UNIQUE(tenant_id,operation_id,action)
);

CREATE TABLE IF NOT EXISTS operational_alert_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  alert_id TEXT NOT NULL REFERENCES operational_alerts(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  operation_id TEXT NOT NULL,
  event_type TEXT NOT NULL CHECK(event_type IN ('CREATED','ACKNOWLEDGED','IN_PROGRESS','RESOLVED','DISMISSED')),
  previous_status TEXT CHECK(previous_status IS NULL OR previous_status IN ('NEW','ACKNOWLEDGED','IN_PROGRESS')),
  new_status TEXT NOT NULL CHECK(new_status IN ('NEW','ACKNOWLEDGED','IN_PROGRESS','RESOLVED','DISMISSED')),
  assigned_user_id TEXT REFERENCES users(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  entered_by_user_id TEXT NOT NULL REFERENCES users(id),
  note TEXT,
  created_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id)
);

CREATE INDEX IF NOT EXISTS ix_operational_alerts_active
ON operational_alerts(tenant_id,branch_id,status,severity,created_at,id);
CREATE INDEX IF NOT EXISTS ix_operational_alert_events_history
ON operational_alert_events(tenant_id,alert_id,created_at,id);

CREATE TRIGGER IF NOT EXISTS immutable_alert_operation_results_update BEFORE UPDATE ON alert_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:alert_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_alert_operation_results_delete BEFORE DELETE ON alert_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:alert_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_operational_alert_events_update BEFORE UPDATE ON operational_alert_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:operational_alert_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_operational_alert_events_delete BEFORE DELETE ON operational_alert_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:operational_alert_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_operational_alert_delete BEFORE DELETE ON operational_alerts BEGIN SELECT RAISE(ABORT,'IMMUTABLE:operational_alerts'); END;

CREATE TRIGGER IF NOT EXISTS guard_operational_alert_insert BEFORE INSERT ON operational_alerts
WHEN NEW.status!='NEW' OR NEW.resolved_at IS NOT NULL
 OR NEW.severity NOT IN ('LOW','MEDIUM','HIGH','CRITICAL')
 OR NEW.alert_type NOT IN ('BACKUP_FAILURE','TERMINAL_OFFLINE','SYNC_DELAY','CASH_VARIANCE','NEGATIVE_STOCK','EXPIRY','LOW_STOCK','OVERDUE_SUPPLIER_INVOICE','OVERDUE_CUSTOMER_CREDIT','UNKNOWN_BARCODE','SETTLEMENT_DISCREPANCY','SECURITY_EVENT','OTHER')
 OR json_valid(NEW.details_json)!=1
 OR NEW.branch_id IS NULL
 OR NOT EXISTS (SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR (NEW.assigned_user_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.assigned_user_id AND u.tenant_id=NEW.tenant_id AND u.status='ACTIVE'))
BEGIN SELECT RAISE(ABORT,'INVALID_OPERATIONAL_ALERT'); END;

CREATE TRIGGER IF NOT EXISTS guard_operational_alert_identity_update BEFORE UPDATE ON operational_alerts
WHEN NEW.tenant_id!=OLD.tenant_id OR NEW.branch_id!=OLD.branch_id OR NEW.severity!=OLD.severity
 OR NEW.alert_type!=OLD.alert_type OR NEW.entity_type IS NOT OLD.entity_type OR NEW.entity_id IS NOT OLD.entity_id
 OR NEW.title!=OLD.title OR NEW.details_json!=OLD.details_json OR NEW.created_at!=OLD.created_at
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:operational_alert_identity'); END;

CREATE TRIGGER IF NOT EXISTS guard_operational_alert_transition BEFORE UPDATE OF status,assigned_user_id,resolved_at ON operational_alerts
WHEN OLD.status IN ('RESOLVED','DISMISSED')
 OR NOT ((OLD.status='NEW' AND NEW.status='ACKNOWLEDGED')
      OR (OLD.status='ACKNOWLEDGED' AND NEW.status IN ('IN_PROGRESS','DISMISSED'))
      OR (OLD.status='IN_PROGRESS' AND NEW.status IN ('RESOLVED','DISMISSED')))
 OR ((NEW.status IN ('RESOLVED','DISMISSED')) != (NEW.resolved_at IS NOT NULL))
 OR (NEW.assigned_user_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.assigned_user_id AND u.tenant_id=NEW.tenant_id AND u.status='ACTIVE'))
 OR NOT EXISTS (
   SELECT 1 FROM operational_alert_events e
   WHERE e.alert_id=OLD.id AND e.tenant_id=OLD.tenant_id
     AND e.previous_status=OLD.status AND e.new_status=NEW.status
     AND e.assigned_user_id IS NEW.assigned_user_id
 )
BEGIN SELECT RAISE(ABORT,'INVALID_OPERATIONAL_ALERT_TRANSITION'); END;

CREATE TRIGGER IF NOT EXISTS guard_operational_alert_event_insert BEFORE INSERT ON operational_alert_events
WHEN NOT EXISTS (SELECT 1 FROM operational_alerts a WHERE a.id=NEW.alert_id AND a.tenant_id=NEW.tenant_id AND a.branch_id=NEW.branch_id)
 OR NOT EXISTS (SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id AND d.status='ACTIVE')
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.entered_by_user_id AND u.tenant_id=NEW.tenant_id AND u.status='ACTIVE')
 OR (NEW.assigned_user_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.assigned_user_id AND u.tenant_id=NEW.tenant_id AND u.status='ACTIVE'))
 OR NOT (
   (NEW.event_type='CREATED' AND NEW.previous_status IS NULL AND NEW.new_status='NEW' AND NOT EXISTS (SELECT 1 FROM operational_alert_events e WHERE e.alert_id=NEW.alert_id))
   OR
   (NEW.event_type=NEW.new_status AND NEW.previous_status IS NOT NULL
    AND EXISTS (SELECT 1 FROM operational_alerts a WHERE a.id=NEW.alert_id AND a.status=NEW.previous_status)
    AND ((NEW.previous_status='NEW' AND NEW.new_status='ACKNOWLEDGED')
      OR (NEW.previous_status='ACKNOWLEDGED' AND NEW.new_status IN ('IN_PROGRESS','DISMISSED'))
      OR (NEW.previous_status='IN_PROGRESS' AND NEW.new_status IN ('RESOLVED','DISMISSED')))
   )
 )
 OR (NEW.new_status IN ('RESOLVED','DISMISSED') AND (NEW.note IS NULL OR length(trim(NEW.note))=0))
BEGIN SELECT RAISE(ABORT,'INVALID_OPERATIONAL_ALERT_EVENT'); END;

INSERT INTO permissions(code,description) VALUES('alert.create','Create operational alerts') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('alert.manage','Assign and transition operational alerts') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('alert.view','View operational alerts') ON CONFLICT(code) DO UPDATE SET description=excluded.description;

INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code IN ('alert.create','alert.manage','alert.view')
WHERE lower(r.name) IN ('owner','administrator','admin','manager');
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code='alert.view'
WHERE lower(r.name)='accountant';
