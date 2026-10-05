import { invoke } from '@tauri-apps/api/core';

export type BackupResult = {
  backup_id: string;
  storage_path: string;
  sha256: string;
  byte_size: number;
  schema_version: string;
  integrity_state: string;
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

export const adminApi = {
  createBackup: (operationId: string) =>
    invoke<BackupResult>('create_verified_backup', { request: { operationId } }),
  previewRestore: (backupId: string) =>
    invoke<RestorePreview>('preview_verified_restore', { request: { backupId } }),
  restoreBackup: (operationId: string, backupId: string, expectedSha256: string) =>
    invoke<RestoreResult>('restore_verified_backup', {
      request: { operationId, backupId, expectedSha256 },
    }),
};
