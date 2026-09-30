-- Crash-safe printer leases are isolated from immutable receipt evidence.
CREATE TABLE IF NOT EXISTS print_job_leases (
  print_job_id TEXT PRIMARY KEY REFERENCES print_jobs(id) ON DELETE CASCADE,
  lease_token TEXT NOT NULL UNIQUE,
  leased_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS ix_print_job_leases_age
ON print_job_leases(leased_at,print_job_id);

CREATE TABLE IF NOT EXISTS printer_profiles (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  branch_id TEXT NOT NULL REFERENCES branches(id),
  device_id TEXT NOT NULL REFERENCES devices(id),
  friendly_name TEXT NOT NULL,
  transport TEXT NOT NULL CHECK(transport IN ('WINDOWS_SPOOLER','SERIAL')),
  target TEXT,
  paper_width_mm INTEGER NOT NULL CHECK(paper_width_mm IN (58,80)),
  characters_per_line INTEGER NOT NULL CHECK(characters_per_line BETWEEN 24 AND 64),
  character_encoding TEXT NOT NULL CHECK(character_encoding IN ('ASCII','UTF8')),
  cut_mode TEXT NOT NULL CHECK(cut_mode IN ('NONE','PARTIAL','FULL')),
  drawer_pulse_policy TEXT NOT NULL CHECK(drawer_pulse_policy IN ('NEVER','CASH_SALE')),
  is_default INTEGER NOT NULL DEFAULT 1 CHECK(is_default IN (0,1)),
  active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(tenant_id,branch_id,device_id,friendly_name)
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_printer_profile_default
ON printer_profiles(tenant_id,branch_id,device_id)
WHERE is_default=1 AND active=1;

CREATE TRIGGER IF NOT EXISTS guard_printer_profiles_insert BEFORE INSERT ON printer_profiles
WHEN NOT EXISTS (
  SELECT 1 FROM devices d
  WHERE d.id=NEW.device_id AND d.tenant_id=NEW.tenant_id AND d.branch_id=NEW.branch_id
)
BEGIN SELECT RAISE(ABORT,'TENANT_SCOPE_VIOLATION:printer_profiles'); END;

CREATE TRIGGER IF NOT EXISTS guard_printer_profiles_update BEFORE UPDATE ON printer_profiles
WHEN NEW.tenant_id<>OLD.tenant_id OR NEW.branch_id<>OLD.branch_id OR NEW.device_id<>OLD.device_id
BEGIN SELECT RAISE(ABORT,'IMMUTABLE_SCOPE:printer_profiles'); END;

INSERT OR IGNORE INTO printer_profiles(
  id,tenant_id,branch_id,device_id,friendly_name,transport,target,
  paper_width_mm,characters_per_line,character_encoding,cut_mode,
  drawer_pulse_policy,is_default,active,created_at,updated_at
)
SELECT lower(hex(randomblob(4)))||'-'||lower(hex(randomblob(2)))||'-4'||substr(lower(hex(randomblob(2))),2)||'-a'||substr(lower(hex(randomblob(2))),2)||'-'||lower(hex(randomblob(6))),
       tenant_id,branch_id,device_id,'Default receipt printer','WINDOWS_SPOOLER',NULL,
       80,48,'ASCII','PARTIAL','CASH_SALE',1,1,installed_at,installed_at
FROM local_terminal_binding;
