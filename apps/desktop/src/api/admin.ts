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

export const adminApi = {
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
