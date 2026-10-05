CREATE TABLE IF NOT EXISTS background_job_operation_results (
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  operation_id TEXT NOT NULL,
  action TEXT NOT NULL,
  request_sha256 TEXT NOT NULL,
  result_json TEXT NOT NULL,
  committed_at TEXT NOT NULL,
  PRIMARY KEY(tenant_id,operation_id)
);

CREATE TABLE IF NOT EXISTS background_job_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  job_id TEXT NOT NULL REFERENCES background_jobs(id),
  operation_id TEXT NOT NULL,
  event_type TEXT NOT NULL CHECK(event_type IN (
    'ENQUEUED','CLAIMED','PROGRESS','CANCEL_REQUESTED','CANCELLED','SUCCEEDED',
    'RETRY_SCHEDULED','FAILED','REQUIRES_REVIEW','LEASE_EXPIRED_REQUEUED',
    'LEASE_EXPIRED_FAILED','LEASE_EXPIRED_CANCELLED'
  )),
  previous_state TEXT,
  new_state TEXT NOT NULL CHECK(new_state IN ('QUEUED','RUNNING','SUCCEEDED','FAILED','CANCELLED','REQUIRES_REVIEW')),
  device_id TEXT NOT NULL REFERENCES devices(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  evidence_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(tenant_id,operation_id,job_id,event_type)
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_background_jobs_operation
  ON background_jobs(tenant_id,operation_id) WHERE operation_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_background_jobs_claim
  ON background_jobs(tenant_id,branch_id,state,not_before,retry_after,created_at,id);
CREATE INDEX IF NOT EXISTS ix_background_jobs_lease
  ON background_jobs(tenant_id,branch_id,state,lease_expires_at);
CREATE INDEX IF NOT EXISTS ix_background_job_events_history
  ON background_job_events(tenant_id,job_id,created_at,id);

CREATE TRIGGER IF NOT EXISTS immutable_background_job_operation_results_update
BEFORE UPDATE ON background_job_operation_results
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:background_job_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_background_job_operation_results_delete
BEFORE DELETE ON background_job_operation_results
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:background_job_operation_results'); END;
CREATE TRIGGER IF NOT EXISTS immutable_background_job_events_update
BEFORE UPDATE ON background_job_events
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:background_job_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_background_job_events_delete
BEFORE DELETE ON background_job_events
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:background_job_events'); END;
CREATE TRIGGER IF NOT EXISTS immutable_background_jobs_delete
BEFORE DELETE ON background_jobs
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:background_jobs'); END;

CREATE TRIGGER IF NOT EXISTS guard_background_job_insert
BEFORE INSERT ON background_jobs
WHEN NEW.branch_id IS NULL OR NEW.origin_device_id IS NULL OR NEW.operation_id IS NULL
 OR NEW.request_sha256 IS NULL OR length(NEW.request_sha256)!=64
 OR length(trim(NEW.job_type))=0 OR length(NEW.job_type)>64
 OR NEW.state!='QUEUED' OR NEW.progress_current!=0
 OR NEW.progress_total IS NOT NULL AND NEW.progress_total<0
 OR NEW.cancellable NOT IN (0,1) OR NEW.attempts!=0
 OR NEW.max_attempts<1 OR NEW.max_attempts>10
 OR json_valid(NEW.payload_json)!=1
 OR NEW.lease_token IS NOT NULL OR NEW.lease_owner_device_id IS NOT NULL OR NEW.lease_expires_at IS NOT NULL
 OR NEW.cancel_requested_at IS NOT NULL OR NEW.cancel_requested_by_user_id IS NOT NULL OR NEW.cancel_reason IS NOT NULL
 OR NEW.result_json IS NOT NULL OR NEW.error_text IS NOT NULL OR NEW.retry_after IS NOT NULL
 OR NEW.started_at IS NOT NULL OR NEW.completed_at IS NOT NULL
 OR NOT EXISTS (SELECT 1 FROM branches b WHERE b.id=NEW.branch_id AND b.tenant_id=NEW.tenant_id)
 OR NOT EXISTS (SELECT 1 FROM devices d WHERE d.id=NEW.origin_device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id AND d.status='ACTIVE')
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.created_by_user_id AND u.tenant_id=NEW.tenant_id AND u.status='ACTIVE')
BEGIN SELECT RAISE(ABORT,'INVALID_BACKGROUND_JOB'); END;

CREATE TRIGGER IF NOT EXISTS guard_background_job_identity_update
BEFORE UPDATE ON background_jobs
WHEN NEW.tenant_id!=OLD.tenant_id OR NEW.branch_id IS NOT OLD.branch_id
 OR NEW.job_type!=OLD.job_type OR NEW.payload_json!=OLD.payload_json
 OR NEW.created_by_user_id IS NOT OLD.created_by_user_id OR NEW.created_at!=OLD.created_at
 OR NEW.origin_device_id IS NOT OLD.origin_device_id OR NEW.operation_id IS NOT OLD.operation_id
 OR NEW.request_sha256 IS NOT OLD.request_sha256 OR NEW.cancellable!=OLD.cancellable
 OR NEW.max_attempts!=OLD.max_attempts OR NEW.not_before IS NOT OLD.not_before
BEGIN SELECT RAISE(ABORT,'IMMUTABLE:background_job_identity'); END;

CREATE TRIGGER IF NOT EXISTS guard_background_job_update
BEFORE UPDATE ON background_jobs
WHEN NEW.progress_current<OLD.progress_current OR NEW.progress_current<0
 OR (NEW.progress_total IS NOT NULL AND (NEW.progress_total<NEW.progress_current OR (OLD.progress_total IS NOT NULL AND NEW.progress_total!=OLD.progress_total)))
 OR NEW.attempts<OLD.attempts OR NEW.attempts>OLD.attempts+1 OR NEW.attempts>NEW.max_attempts
 OR NOT (
   (OLD.state='QUEUED' AND NEW.state='RUNNING' AND NEW.attempts=OLD.attempts+1)
   OR (OLD.state='QUEUED' AND NEW.state='CANCELLED' AND NEW.attempts=OLD.attempts)
   OR (OLD.state='RUNNING' AND NEW.state='RUNNING' AND NEW.attempts=OLD.attempts)
   OR (OLD.state='RUNNING' AND NEW.state='QUEUED' AND NEW.attempts=OLD.attempts)
   OR (OLD.state='RUNNING' AND NEW.state IN ('SUCCEEDED','FAILED','CANCELLED','REQUIRES_REVIEW') AND NEW.attempts=OLD.attempts)
 )
 OR ((NEW.state='RUNNING') != (NEW.lease_token IS NOT NULL AND NEW.lease_owner_device_id IS NOT NULL AND NEW.lease_expires_at IS NOT NULL))
 OR (NEW.state!='RUNNING' AND (NEW.lease_token IS NOT NULL OR NEW.lease_owner_device_id IS NOT NULL OR NEW.lease_expires_at IS NOT NULL))
 OR (NEW.state IN ('SUCCEEDED','FAILED','CANCELLED','REQUIRES_REVIEW')) != (NEW.completed_at IS NOT NULL)
 OR (NEW.state NOT IN ('SUCCEEDED','FAILED','CANCELLED','REQUIRES_REVIEW') AND NEW.completed_at IS NOT NULL)
 OR (NEW.state='SUCCEEDED' AND (NEW.result_json IS NULL OR json_valid(NEW.result_json)!=1))
 OR (NEW.state IN ('FAILED','REQUIRES_REVIEW') AND (NEW.error_text IS NULL OR length(trim(NEW.error_text))=0))
 OR (NEW.state='CANCELLED' AND (NEW.cancellable!=1 OR NEW.cancel_requested_at IS NULL OR NEW.cancel_requested_by_user_id IS NULL OR NEW.cancel_reason IS NULL OR length(trim(NEW.cancel_reason))=0))
 OR (NEW.cancel_requested_at IS NOT NULL AND (NEW.cancellable!=1 OR NEW.cancel_requested_by_user_id IS NULL OR NEW.cancel_reason IS NULL OR length(trim(NEW.cancel_reason))=0))
 OR (NEW.state='QUEUED' AND NEW.attempts>0 AND (NEW.error_text IS NULL OR NEW.retry_after IS NULL))
BEGIN SELECT RAISE(ABORT,'INVALID_BACKGROUND_JOB_TRANSITION'); END;

CREATE TRIGGER IF NOT EXISTS guard_background_job_event_insert
BEFORE INSERT ON background_job_events
WHEN json_valid(NEW.evidence_json)!=1
 OR NOT EXISTS (SELECT 1 FROM background_jobs j WHERE j.id=NEW.job_id AND j.tenant_id=NEW.tenant_id AND j.branch_id=NEW.branch_id)
 OR NOT EXISTS (SELECT 1 FROM devices d WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id)
 OR NOT EXISTS (SELECT 1 FROM users u WHERE u.id=NEW.user_id AND u.tenant_id=NEW.tenant_id)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:background_job_event'); END;

INSERT INTO permissions(code,description) VALUES('job.enqueue','Enqueue background jobs') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('job.execute','Execute background jobs') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('job.manage','Cancel and recover background jobs') ON CONFLICT(code) DO UPDATE SET description=excluded.description;
INSERT INTO permissions(code,description) VALUES('job.view','View background jobs') ON CONFLICT(code) DO UPDATE SET description=excluded.description;

INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code IN ('job.enqueue','job.execute','job.manage','job.view')
WHERE lower(r.name) IN ('owner','administrator','admin','manager');
INSERT OR IGNORE INTO role_permissions(role_id,permission_code)
SELECT r.id,p.code FROM roles r JOIN permissions p ON p.code IN ('job.execute','job.view')
WHERE lower(r.name) IN ('accountant','inventory staff');
