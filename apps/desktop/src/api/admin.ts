import { invoke } from '@tauri-apps/api/core';

export type BackgroundJobResult = {
  job_id: string;
  state: string;
  progress_current: number;
  progress_total: number | null;
  attempts: number;
  max_attempts: number;
  cancel_requested: boolean;
  retry_after: string | null;
  error: string | null;
  updated_at: string;
};

export type RestorePreview = {
  backup_id: string;
  sha256: string;
  byte_size: number;
  schema_version: string;
  integrity_state: string;
  compatible: boolean;
};

export type RestoreResult = {
  restore_id: string;
  backup_id: string;
  pre_restore_backup_id: string;
  restored_sha256: string;
  completed_at: string;
};

export type BackupScheduleResult = {
  schedule_id: string;
  state: 'ACTIVE' | 'DISABLED' | 'REQUIRES_REVIEW';
  interval_minutes: number;
  retention_count: number;
  next_run_at: string;
  authorization_expires_at: string;
  version: number;
};

export type OperationalDiagnostics = {
  tenant_id: string;
  branch_id: string;
  device_id: string;
  register_id: string;
  schema_version: string;
  database_integrity: string;
  foreign_key_violations: number;
  device_status: string;
  device_app_version: string | null;
  last_heartbeat_at: string | null;
  pending_sync_mutations: number;
  sync_requires_review: number;
  last_sync_at: string | null;
  pending_background_jobs: number;
  background_jobs_requires_review: number;
  failed_print_jobs: number;
  printer_configured: boolean;
  latest_backup: null | { backup_id:string; backup_type:string; state:string; integrity_state:string|null; created_at:string };
  backup_schedule: null | { state:string; next_run_at:string; authorization_expires_at:string; retention_count:number };
  application_version: string;
  build_sha: string | null;
  database_path: string;
  backup_directory: string;
  hub_mode: string;
  whatsapp_status: string;
  ocr_status: string;
};

export const adminApi = {
  diagnostics: () => invoke<OperationalDiagnostics>('get_operational_diagnostics'),
  createBackup: (operationId: string) =>
    invoke<BackgroundJobResult>('create_verified_backup', { request: { operationId } }),
  configureBackupSchedule: (
    operationId: string,
    intervalMinutes: number,
    retentionCount: number,
    enabled: boolean,
    firstRunAt: string,
    authorizationValidDays: number,
  ) =>
    invoke<BackupScheduleResult>('configure_backup_schedule', {
      request: {
        operationId,
        intervalMinutes,
        retentionCount,
        enabled,
        firstRunAt,
        authorizationValidDays,
      },
    }),
  previewRestore: (backupId: string) =>
    invoke<RestorePreview>('preview_verified_restore', { request: { backupId } }),
  restoreBackup: (operationId: string, backupId: string, expectedSha256: string) =>
    invoke<RestoreResult>('restore_verified_backup', {
      request: { operationId, backupId, expectedSha256 },
    }),
};
