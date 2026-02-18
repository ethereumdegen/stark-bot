import { apiFetch } from './core';

// Logs API
export async function getLogs(limit?: number): Promise<Array<{
  id: string;
  level: string;
  message: string;
  timestamp: string;
}>> {
  const query = limit ? `?limit=${limit}` : '';
  return apiFetch(`/logs${query}`);
}

// System Info API
export interface SystemInfoResponse {
  disk: {
    enabled: boolean;
    used_bytes: number;
    quota_bytes: number;
    remaining_bytes: number;
    percentage: number;
    breakdown: Record<string, number>;
  };
  uptime_secs: number;
  version: string;
}

export interface CleanupResult {
  success: boolean;
  deleted_count: number;
  freed_bytes: number;
  error?: string;
}

export async function getSystemInfo(): Promise<SystemInfoResponse> {
  return apiFetch('/system/info');
}

export async function cleanupMemories(olderThanDays: number): Promise<CleanupResult> {
  return apiFetch('/system/cleanup/memories', {
    method: 'POST',
    body: JSON.stringify({ older_than_days: olderThanDays }),
  });
}

export async function cleanupWorkspace(): Promise<CleanupResult> {
  return apiFetch('/system/cleanup/workspace', {
    method: 'POST',
    body: JSON.stringify({ confirm: true }),
  });
}
