import { apiFetch } from './core';

// Cron Jobs API
export interface CronJobInfo {
  id: number;
  job_id: string;
  name: string;
  description?: string;
  schedule_type: string;
  schedule_value: string;
  timezone?: string;
  session_mode: string;
  message?: string;
  system_event?: string;
  channel_id?: number;
  deliver_to?: string;
  deliver: boolean;
  model_override?: string;
  thinking_level?: string;
  timeout_seconds?: number;
  delete_after_run: boolean;
  status: string;
  last_run_at?: string;
  next_run_at?: string;
  run_count: number;
  error_count: number;
  last_error?: string;
  created_at: string;
  updated_at: string;
}

interface CronJobResponse {
  success: boolean;
  job?: CronJobInfo;
  jobs?: CronJobInfo[];
  error?: string;
}

export async function getCronJobs(): Promise<CronJobInfo[]> {
  const response = await apiFetch<CronJobResponse>('/cron/jobs');
  return response.jobs || [];
}

export async function getCronJob(id: number): Promise<CronJobInfo | null> {
  const response = await apiFetch<CronJobResponse>(`/cron/jobs/${id}`);
  return response.job || null;
}

export async function createCronJob(data: {
  name: string;
  description?: string;
  schedule_type: string;
  schedule_value: string;
  timezone?: string;
  session_mode: string;
  message?: string;
  system_event?: string;
  channel_id?: number;
  deliver_to?: string;
  deliver?: boolean;
  model_override?: string;
  thinking_level?: string;
  timeout_seconds?: number;
  delete_after_run?: boolean;
}): Promise<CronJobInfo> {
  const response = await apiFetch<CronJobResponse>('/cron/jobs', {
    method: 'POST',
    body: JSON.stringify(data),
  });
  if (!response.success || !response.job) {
    throw new Error(response.error || 'Failed to create cron job');
  }
  return response.job;
}

export async function updateCronJob(id: number, data: Partial<{
  name: string;
  description: string;
  schedule_type: string;
  schedule_value: string;
  timezone: string;
  session_mode: string;
  message: string;
  system_event: string;
  channel_id: number;
  deliver_to: string;
  deliver: boolean;
  model_override: string;
  thinking_level: string;
  timeout_seconds: number;
  delete_after_run: boolean;
  status: string;
}>): Promise<CronJobInfo> {
  const response = await apiFetch<CronJobResponse>(`/cron/jobs/${id}`, {
    method: 'PUT',
    body: JSON.stringify(data),
  });
  if (!response.success || !response.job) {
    throw new Error(response.error || 'Failed to update cron job');
  }
  return response.job;
}

export async function deleteCronJob(id: number): Promise<void> {
  const response = await apiFetch<CronJobResponse>(`/cron/jobs/${id}`, {
    method: 'DELETE',
  });
  if (!response.success) {
    throw new Error(response.error || 'Failed to delete cron job');
  }
}

export async function runCronJobNow(id: number): Promise<void> {
  const response = await apiFetch<CronJobResponse>(`/cron/jobs/${id}/run`, {
    method: 'POST',
  });
  if (!response.success) {
    throw new Error(response.error || 'Failed to run cron job');
  }
}

export async function pauseCronJob(id: number): Promise<CronJobInfo> {
  const response = await apiFetch<CronJobResponse>(`/cron/jobs/${id}/pause`, {
    method: 'POST',
  });
  if (!response.success || !response.job) {
    throw new Error(response.error || 'Failed to pause cron job');
  }
  return response.job;
}

export async function resumeCronJob(id: number): Promise<CronJobInfo> {
  const response = await apiFetch<CronJobResponse>(`/cron/jobs/${id}/resume`, {
    method: 'POST',
  });
  if (!response.success || !response.job) {
    throw new Error(response.error || 'Failed to resume cron job');
  }
  return response.job;
}

export interface CronJobRunInfo {
  id: number;
  cron_job_id: number;
  started_at: string;
  completed_at?: string;
  success: boolean;
  response?: string;
  error?: string;
  duration_ms?: number;
}

export async function getCronJobRuns(id: number, limit?: number): Promise<CronJobRunInfo[]> {
  const query = limit ? `?limit=${limit}` : '';
  const response = await apiFetch<{ success: boolean; runs?: CronJobRunInfo[] }>(`/cron/jobs/${id}/runs${query}`);
  return response.runs || [];
}

// Heartbeat Config API
export interface HeartbeatConfigInfo {
  id: number;
  channel_id?: number;
  interval_minutes: number;
  target?: string;
  active_hours_start?: string;
  active_hours_end?: string;
  active_days?: string;
  enabled: boolean;
  last_beat_at?: string;
  next_beat_at?: string;
  created_at: string;
  updated_at: string;
}

interface HeartbeatConfigResponse {
  success: boolean;
  config?: HeartbeatConfigInfo;
  error?: string;
}

export async function getHeartbeatConfig(): Promise<HeartbeatConfigInfo | null> {
  const response = await apiFetch<HeartbeatConfigResponse>('/heartbeat/config');
  return response.config || null;
}

export async function updateHeartbeatConfig(data: {
  interval_minutes?: number;
  target?: string;
  active_hours_start?: string;
  active_hours_end?: string;
  active_days?: string;
  enabled?: boolean;
}): Promise<HeartbeatConfigInfo> {
  const response = await apiFetch<HeartbeatConfigResponse>('/heartbeat/config', {
    method: 'PUT',
    body: JSON.stringify(data),
  });
  if (!response.success || !response.config) {
    throw new Error(response.error || 'Failed to update heartbeat config');
  }
  return response.config;
}

export async function pulseHeartbeatOnce(): Promise<HeartbeatConfigInfo> {
  const response = await apiFetch<HeartbeatConfigResponse>('/heartbeat/pulse_once', {
    method: 'POST',
  });
  if (!response.success || !response.config) {
    throw new Error(response.error || 'Failed to pulse heartbeat');
  }
  return response.config;
}
