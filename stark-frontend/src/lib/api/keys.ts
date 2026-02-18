import { apiFetch } from './core';

// API Keys API
export interface KeyConfig {
  name: string;
  label: string;
  secret: boolean;
}

export interface ServiceConfig {
  group: string;
  label: string;
  description: string;
  url: string;
  keys: KeyConfig[];
}

export interface ServiceConfigsResponse {
  success: boolean;
  configs: ServiceConfig[];
}

export interface ApiKey {
  id: number;
  key_name: string;
  key_preview: string;
  is_secret: boolean;
  created_at: string;
  updated_at: string;
}

export interface ApiKeysResponse {
  success: boolean;
  keys?: ApiKey[];
  error?: string;
}

export async function getServiceConfigs(): Promise<ServiceConfig[]> {
  const response = await apiFetch<ServiceConfigsResponse>('/keys/config');
  return response.configs || [];
}

export async function getApiKeys(): Promise<ApiKey[]> {
  const response = await apiFetch<ApiKeysResponse>('/keys');
  return response.keys || [];
}

export async function upsertApiKey(keyName: string, apiKey: string): Promise<void> {
  await apiFetch('/keys', {
    method: 'POST',
    body: JSON.stringify({ key_name: keyName, api_key: apiKey }),
  });
}

export async function deleteApiKey(keyName: string): Promise<void> {
  await apiFetch('/keys', {
    method: 'DELETE',
    body: JSON.stringify({ key_name: keyName }),
  });
}

export async function getApiKeyValue(keyName: string): Promise<string> {
  const response = await apiFetch<{ success: boolean; key_value?: string; error?: string }>(
    `/keys/value?key_name=${encodeURIComponent(keyName)}`
  );
  if (!response.success || !response.key_value) {
    throw new Error(response.error || 'Failed to get API key value');
  }
  return response.key_value;
}

// Cloud Backup API
export interface BackupResponse {
  success: boolean;
  key_count?: number;
  node_count?: number;
  connection_count?: number;
  cron_job_count?: number;
  channel_count?: number;
  channel_setting_count?: number;
  discord_registration_count?: number;
  skill_count?: number;
  agent_settings_count?: number;
  has_settings?: boolean;
  has_heartbeat?: boolean;
  has_soul?: boolean;
  message?: string;
  error?: string;
}

export interface CloudKeyPreview {
  key_name: string;
  key_preview: string;
}

export interface CloudBackupPreview {
  success: boolean;
  key_count: number;
  keys: CloudKeyPreview[];
  node_count?: number;
  connection_count?: number;
  cron_job_count?: number;
  channel_count?: number;
  channel_setting_count?: number;
  discord_registration_count?: number;
  skill_count?: number;
  agent_settings_count?: number;
  has_settings?: boolean;
  has_heartbeat?: boolean;
  has_soul?: boolean;
  backup_version?: number;
  message?: string;
  error?: string;
}

// Legacy alias for backwards compatibility
export type PreviewKeysResponse = CloudBackupPreview;

export async function backupKeysToCloud(): Promise<BackupResponse> {
  const response = await apiFetch<BackupResponse>('/keys/cloud_backup', {
    method: 'POST',
  });
  if (!response.success) {
    throw new Error(response.error || 'Failed to backup');
  }
  return response;
}

export async function restoreKeysFromCloud(): Promise<BackupResponse> {
  const response = await apiFetch<BackupResponse>('/keys/cloud_restore', {
    method: 'POST',
  });
  if (!response.success) {
    throw new Error(response.error || 'Failed to restore');
  }
  return response;
}

export async function previewCloudBackup(): Promise<CloudBackupPreview> {
  const response = await apiFetch<CloudBackupPreview>('/keys/cloud_preview', {
    method: 'GET',
  });
  if (!response.success) {
    throw new Error(response.error || 'Failed to preview cloud backup');
  }
  return response;
}

// Legacy alias
export const previewKeysFromCloud = previewCloudBackup;
