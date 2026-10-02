CREATE TABLE IF NOT EXISTS employee_operation_results (
  tenant_id TEXT NOT NULL REFERENCES tenants(id), operation_id TEXT NOT NULL,
  action TEXT NOT NULL, request_sha256 TEXT NOT NULL, result_json TEXT NOT NULL, committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id), UNIQUE(tenant_id,operation_id,action)
);

CREATE TABLE IF NOT EXISTS attendance_operation_results (
  tenant_id TEXT NOT NULL REFERENCES tenants(id), operation_id TEXT NOT NULL,
  action TEXT NOT NULL, request_sha256 TEXT NOT NULL, result_json TEXT NOT NULL, committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id), UNIQUE(tenant_id,operation_id,action)
);

CREATE TABLE IF NOT EXISTS attendance_sessions (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  employee_id TEXT NOT NULL REFERENCES employees(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  state TEXT NOT NULL CHECK(state IN ('CLOCKED_IN','ON_BREAK','CLOCKED_OUT','MISSING_CLOCK_OUT')),
  clocked_in_at TEXT NOT NULL,
  clocked_out_at TEXT,
  active_break_started_at TEXT,
  break_seconds INTEGER NOT NULL DEFAULT 0,
  worked_seconds INTEGER,
  created_device_id TEXT NOT NULL REFERENCES devices(id),
  created_by_user_id TEXT NOT NULL REFERENCES users(id),
  last_operation_id TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_attendance_active_employee
ON attendance_sessions(tenant_id,employee_id) WHERE state IN ('CLOCKED_IN','ON_BREAK');
CREATE INDEX IF NOT EXISTS ix_attendance_report
ON attendance_sessions(tenant_id,branch_id,employee_id,clocked_in_at,id);

CREATE TABLE IF NOT EXISTS attendance_session_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  session_id TEXT NOT NULL REFERENCES attendance_sessions(id),
  employee_id TEXT NOT NULL REFERENCES employees(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  operation_id TEXT NOT NULL,
  event_type TEXT NOT NULL CHECK(event_type IN ('CLOCK_IN','BREAK_START','BREAK_END','CLOCK_OUT','MISSING_CLOCK_OUT')),
  previous_state TEXT,
  new_state TEXT NOT NULL CHECK(new_state IN ('CLOCKED_IN','ON_BREAK','CLOCKED_OUT','MISSING_CLOCK_OUT')),
  occurred_at TEXT NOT NULL,
  device_id TEXT NOT NULL REFERENCES devices(id),
  entered_by_user_id TEXT NOT NULL REFERENCES users(id),
  note TEXT,
  created_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id)
);

CREATE INDEX IF NOT EXISTS ix_attendance_session_events
ON attendance_session_events(tenant_id,session_id,occurred_at,id);

CREATE TRIGGER IF NOT EXISTS immutable_employee_operation_results_update BEFORE UPDATE ON employee_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:employee_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_employee_operation_results_delete BEFORE DELETE ON employee_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:employee_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_attendance_operation_results_update BEFORE UPDATE ON attendance_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:attendance_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_attendance_operation_results_delete BEFORE DELETE ON attendance_operation_results BEGIN SELECT RAISE(ABORT,'IMMUTABLE:attendance_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_attendance_session_events_update BEFORE UPDATE ON attendance_session_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:attendance_session_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_attendance_session_events_delete BEFORE DELETE ON attendance_session_events BEGIN SELECT RAISE(ABORT,'IMMUTABLE:attendance_session_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_closed_attendance_session_update BEFORE UPDATE ON attendance_sessions
WHEN OLD.state IN ('CLOCKED_OUT','MISSING_CLOCK_OUT') BEGIN SELECT RAISE(ABORT,'IMMUTABLE:closed_attendance_session'); END;
CREATE TRIGGER IF NOT EXISTS immutable_attendance_session_delete BEFORE DELETE ON attendance_sessions BEGIN SELECT RAISE(ABORT,'IMMUTABLE:attendance_sessions'); END;

CREATE TRIGGER IF NOT EXISTS guard_employee_branch_insert BEFORE INSERT ON employee_branches
WHEN NOT EXISTS (SELECT 1 FROM employees e JOIN branches b ON b.id=NEW.branch_id WHERE e.id=NEW.employee_id AND e.tenant_id=b.tenant_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:employee_branches'); END;

CREATE TRIGGER IF NOT EXISTS guard_attendance_session_insert BEFORE INSERT ON attendance_sessions
WHEN NEW.state!='CLOCKED_IN' OR NEW.clocked_out_at IS NOT NULL OR NEW.active_break_started_at IS NOT NULL
 OR typeof(NEW.break_seconds)!='integer' OR NEW.break_seconds!=0 OR NEW.worked_seconds IS NOT NULL
 OR NOT EXISTS (SELECT 1 FROM employees e JOIN employee_branches eb ON eb.employee_id=e.id WHERE e.id=NEW.employee_id AND e.tenant_id=NEW.tenant_id AND e.status='ACTIVE' AND eb.branch_id=NEW.branch_id)
 OR NOT EXISTS (SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (SELECT 1 FROM devices d WHERE d.id=NEW.created_device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id AND d.status='ACTIVE')
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.created_by_user_id AND u.tenant_id=NEW.tenant_id AND u.status='ACTIVE')
BEGIN SELECT RAISE(ABORT,'INVALID_ATTENDANCE_SESSION'); END;

CREATE TRIGGER IF NOT EXISTS guard_attendance_session_update BEFORE UPDATE OF state,clocked_out_at,active_break_started_at,break_seconds,worked_seconds,last_operation_id ON attendance_sessions
WHEN typeof(NEW.break_seconds)!='integer' OR NEW.break_seconds<0
 OR (NEW.worked_seconds IS NOT NULL AND (typeof(NEW.worked_seconds)!='integer' OR NEW.worked_seconds<0))
 OR NOT EXISTS (
   SELECT 1 FROM attendance_session_events e
   WHERE e.session_id=OLD.id AND e.tenant_id=OLD.tenant_id AND e.operation_id=NEW.last_operation_id
     AND e.previous_state=OLD.state AND e.new_state=NEW.state
 )
BEGIN SELECT RAISE(ABORT,'INVALID_ATTENDANCE_TRANSITION'); END;

CREATE TRIGGER IF NOT EXISTS guard_attendance_event_insert BEFORE INSERT ON attendance_session_events
WHEN NOT EXISTS (
   SELECT 1 FROM attendance_sessions s
   WHERE s.id=NEW.session_id AND s.tenant_id=NEW.tenant_id AND s.employee_id=NEW.employee_id AND s.branch_id=NEW.branch_id
 )
 OR NOT EXISTS (SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id AND d.status='ACTIVE')
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.entered_by_user_id AND u.tenant_id=NEW.tenant_id AND u.status='ACTIVE')
 OR EXISTS (SELECT 1 FROM attendance_session_events prior WHERE prior.session_id=NEW.session_id AND prior.occurred_at>=NEW.occurred_at)
 OR NOT (
   (NEW.event_type='CLOCK_IN' AND NEW.previous_state IS NULL AND NEW.new_state='CLOCKED_IN' AND NOT EXISTS (SELECT 1 FROM attendance_session_events prior WHERE prior.session_id=NEW.session_id)) OR
   (NEW.event_type='BREAK_START' AND NEW.previous_state='CLOCKED_IN' AND NEW.new_state='ON_BREAK' AND EXISTS (SELECT 1 FROM attendance_sessions s WHERE s.id=NEW.session_id AND s.state='CLOCKED_IN')) OR
   (NEW.event_type='BREAK_END' AND NEW.previous_state='ON_BREAK' AND NEW.new_state='CLOCKED_IN' AND EXISTS (SELECT 1 FROM attendance_sessions s WHERE s.id=NEW.session_id AND s.state='ON_BREAK')) OR
   (NEW.event_type='CLOCK_OUT' AND NEW.previous_state IN ('CLOCKED_IN','ON_BREAK') AND NEW.new_state='CLOCKED_OUT' AND EXISTS (SELECT 1 FROM attendance_sessions s WHERE s.id=NEW.session_id AND s.state=NEW.previous_state)) OR
   (NEW.event_type='MISSING_CLOCK_OUT' AND NEW.previous_state IN ('CLOCKED_IN','ON_BREAK') AND NEW.new_state='MISSING_CLOCK_OUT' AND NEW.note IS NOT NULL AND length(trim(NEW.note))>0 AND EXISTS (SELECT 1 FROM attendance_sessions s WHERE s.id=NEW.session_id AND s.state=NEW.previous_state))
 )
BEGIN SELECT RAISE(ABORT,'INVALID_ATTENDANCE_EVENT_STATE'); END;

INSERT INTO permissions(code,description) VALUES('employee.manage','Manage employee personnel records') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('attendance.clock','Record employee clock and break events') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('attendance.manage','Resolve attendance exceptions') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('attendance.view','View attendance reports') ON CONFLICT(code) DO UPDATE SET description=excluded.description;

INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code IN ('employee.manage','attendance.clock','attendance.manage','attendance.view')
WHERE lower(r.name) IN ('owner','administrator','admin','manager');
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code='attendance.view'
WHERE lower(r.name)='accountant';
