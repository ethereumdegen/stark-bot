const API_BASE = '/api';

// Config Status API (unauthenticated)
export interface ConfigStatus {
  login_configured: boolean;
  burner_wallet_configured: boolean;
  guest_dashboard_enabled: boolean;
}

export async function getConfigStatus(): Promise<ConfigStatus> {
  const response = await fetch(`${API_BASE}/health/config`);
  if (!response.ok) throw new Error('Failed to fetch config status');
  return response.json();
}

export async function apiFetch<T>(
  endpoint: string,
  options: RequestInit = {}
): Promise<T> {
  const token = localStorage.getItem('stark_token');

  const headers: HeadersInit = {
    'Content-Type': 'application/json',
    ...options.headers,
  };

  if (token) {
    (headers as Record<string, string>)['Authorization'] = `Bearer ${token}`;
  }

  const response = await fetch(`${API_BASE}${endpoint}`, {
    ...options,
    headers,
  });

  if (!response.ok) {
    if (response.status === 401) {
      localStorage.removeItem('stark_token');
      window.location.href = '/';
      throw new Error('Unauthorized');
    }
    const errorText = await response.text();
    throw new Error(errorText || `HTTP ${response.status}`);
  }

  // Handle empty responses
  const text = await response.text();
  if (!text) {
    return {} as T;
  }

  return JSON.parse(text);
}

// Auth API - SIWE (Sign In With Ethereum)
export async function generateChallenge(publicAddress: string): Promise<{ challenge: string }> {
  const response = await fetch(`${API_BASE}/auth/generate_challenge`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ public_address: publicAddress }),
  });

  if (!response.ok) {
    const data = await response.json();
    throw new Error(data.error || 'Failed to generate challenge');
  }

  const data = await response.json();
  if (!data.success || !data.challenge) {
    throw new Error(data.error || 'Failed to generate challenge');
  }

  return { challenge: data.challenge };
}

export async function validateAuth(
  publicAddress: string,
  challenge: string,
  signature: string
): Promise<{ token: string; expires_at: number }> {
  const response = await fetch(`${API_BASE}/auth/validate_auth`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      public_address: publicAddress,
      challenge,
      signature,
    }),
  });

  const data = await response.json();

  if (!response.ok || !data.success) {
    throw new Error(data.error || 'Authentication failed');
  }

  return { token: data.token, expires_at: data.expires_at };
}

export async function validateToken(): Promise<{ valid: boolean }> {
  return apiFetch('/auth/validate');
}

export async function logout(): Promise<void> {
  await apiFetch('/auth/logout', { method: 'POST' });
  localStorage.removeItem('stark_token');
}

// Chat API
export async function sendChatMessage(
  content: string,
  conversationHistory: Array<{ role: string; content: string }>,
  network?: string  // The currently selected network from the UI
): Promise<{ response: string }> {
  // Backend expects { messages: [...] } with the full conversation including the new message
  const messages = [
    ...conversationHistory,
    { role: 'user', content }
  ];

  const response = await apiFetch<{ success: boolean; message?: { content: string }; error?: string }>('/chat', {
    method: 'POST',
    body: JSON.stringify({ messages, network }),
  });

  if (!response.success || !response.message) {
    throw new Error(response.error || 'Failed to get response');
  }

  return { response: response.message.content };
}

// Agent Settings API
export async function getAgentSettings(): Promise<Record<string, unknown>> {
  return apiFetch('/agent-settings');
}

export async function updateAgentSettings(settings: Record<string, unknown>): Promise<void> {
  await apiFetch('/agent-settings', {
    method: 'PUT',
    body: JSON.stringify(settings),
  });
}

// Tools API
interface ToolInfo {
  name: string;
  description: string;
  group: string;
  enabled: boolean;
}

interface ToolsListResponse {
  success: boolean;
  tools?: ToolInfo[];
  error?: string;
}

export async function getTools(): Promise<ToolInfo[]> {
  const response = await apiFetch<ToolsListResponse>('/tools');
  return response.tools || [];
}

export interface ToolGroupInfo {
  key: string;
  label: string;
  description: string;
}

interface ToolGroupsResponse {
  success: boolean;
  groups: ToolGroupInfo[];
}

export async function getToolGroups(): Promise<ToolGroupInfo[]> {
  const response = await apiFetch<ToolGroupsResponse>('/tools/groups');
  return response.groups || [];
}

export async function updateToolEnabled(name: string, enabled: boolean): Promise<void> {
  await apiFetch(`/tools/${encodeURIComponent(name)}/enabled`, {
    method: 'PUT',
    body: JSON.stringify({ enabled }),
  });
}

// Skills API
export interface SkillInfo {
  name: string;
  description: string;
  version: string;
  source: string;
  enabled: boolean;
  requires_tools: string[];
  requires_binaries: string[];
  tags: string[];
  homepage?: string;
  metadata?: string;
}

export async function getSkills(): Promise<SkillInfo[]> {
  return apiFetch('/skills');
}

export async function uploadSkill(file: File): Promise<void> {
  const token = localStorage.getItem('stark_token');
  const formData = new FormData();
  formData.append('file', file);

  const response = await fetch(`${API_BASE}/skills/upload`, {
    method: 'POST',
    headers: token ? { Authorization: `Bearer ${token}` } : {},
    body: formData,
  });

  if (!response.ok) {
    throw new Error('Failed to upload skill');
  }
}

export async function deleteSkill(id: string): Promise<void> {
  await apiFetch(`/skills/${id}`, { method: 'DELETE' });
}

export async function setSkillEnabled(name: string, enabled: boolean): Promise<void> {
  await apiFetch(`/skills/${encodeURIComponent(name)}/enabled`, {
    method: 'PUT',
    body: JSON.stringify({ enabled }),
  });
}

// Sessions API
export async function getSessions(): Promise<Array<{
  id: number;
  channel_type: string;
  channel_id: number;
  platform_chat_id?: string;
  is_active?: boolean;
  completion_status?: string;
  created_at: string;
  updated_at: string;
  message_count?: number;
  initial_query?: string;
  safe_mode?: boolean;
}>> {
  return apiFetch('/sessions');
}

export async function getSession(id: number): Promise<{
  id: number;
  channel_type: string;
  channel_id: number;
  platform_chat_id?: string;
  is_active?: boolean;
  completion_status?: string;
  created_at: string;
  updated_at: string;
  message_count?: number;
  initial_query?: string;
  safe_mode?: boolean;
}> {
  return apiFetch(`/sessions/${id}`);
}

export async function deleteSession(id: string): Promise<{
  success: boolean;
  message: string;
  cancelled_agents?: number;
}> {
  return apiFetch(`/sessions/${id}`, { method: 'DELETE' });
}

export async function deleteAllSessions(): Promise<{
  success: boolean;
  message: string;
  deleted_count: number;
  cancelled_agents: number;
}> {
  return apiFetch('/sessions', { method: 'DELETE' });
}

// Get or create a session by channel type and ID
export async function getOrCreateSession(
  channelType: string,
  channelId: number,
  platformChatId: string
): Promise<{
  id: number;
  channel_type: string;
  channel_id: number;
  created_at: string;
  updated_at: string;
  message_count?: number;
}> {
  return apiFetch('/sessions', {
    method: 'POST',
    body: JSON.stringify({
      channel_type: channelType,
      channel_id: channelId,
      platform_chat_id: platformChatId,
    }),
  });
}

// Reset a session (marks old as inactive, creates new one with same settings)
export async function resetSession(id: number): Promise<{
  id: number;
  channel_type: string;
  channel_id: number;
  is_active: boolean;
  completion_status: string;
  created_at: string;
  updated_at: string;
}> {
  return apiFetch(`/sessions/${id}/reset`, { method: 'POST' });
}

// Stop a session (cancels execution and marks as cancelled)
export async function stopSession(id: number): Promise<{
  success: boolean;
  session?: {
    id: number;
    completion_status: string;
  };
  cancelled_agents?: number;
  error?: string;
}> {
  return apiFetch(`/sessions/${id}/stop`, { method: 'POST' });
}

// Resume a session (marks as active so it can continue processing)
export async function resumeSession(id: number): Promise<{
  success: boolean;
  session?: {
    id: number;
    completion_status: string;
  };
  error?: string;
}> {
  return apiFetch(`/sessions/${id}/resume`, { method: 'POST' });
}

// Web session response type
export interface WebSessionInfo {
  session_id: number;
  completion_status: string;
  message_count: number | null;
  created_at: string;
}

// Get the current active web chat session from the backend
// The backend tracks which session is active for the current user
export async function getActiveWebSession(): Promise<WebSessionInfo | null> {
  const response = await apiFetch<{
    success: boolean;
    session_id?: number;
    completion_status?: string;
    message_count?: number;
    created_at?: string;
    error?: string;
  }>('/chat/session');

  if (response.success && response.session_id) {
    return {
      session_id: response.session_id,
      completion_status: response.completion_status || 'active',
      message_count: response.message_count ?? null,
      created_at: response.created_at || new Date().toISOString(),
    };
  }
  return null;
}

// Create a new web session (resets the current one)
export async function createNewWebSession(): Promise<WebSessionInfo | null> {
  const response = await apiFetch<{
    success: boolean;
    session_id?: number;
    completion_status?: string;
    message_count?: number;
    created_at?: string;
    error?: string;
  }>('/chat/session/new', { method: 'POST' });

  if (response.success && response.session_id) {
    return {
      session_id: response.session_id,
      completion_status: response.completion_status || 'active',
      message_count: response.message_count ?? 0,
      created_at: response.created_at || new Date().toISOString(),
    };
  }
  return null;
}

// Legacy: Get the web chat session from sessions list (fallback)
export async function getWebSession(): Promise<{
  id: number;
  channel_type: string;
  channel_id: number;
  is_active?: boolean;
  completion_status?: string;
  created_at: string;
  updated_at: string;
  message_count?: number;
} | null> {
  // Find the active web session
  const sessions = await getSessions();
  // Prefer active session, fall back to any web session
  const activeWebSession = sessions.find(s => s.channel_type === 'web' && s.channel_id === 0 && s.is_active !== false);
  const webSession = activeWebSession || sessions.find(s => s.channel_type === 'web' && s.channel_id === 0);
  return webSession || null;
}

// Memories API - Enhanced (Phase 5)
export interface MemoryInfo {
  id: number;
  memory_type: string;
  content: string;
  category?: string;
  tags?: string;
  importance: number;
  identity_id?: string;
  source_channel_type?: string;
  log_date?: string;
  created_at: string;
  updated_at: string;
  // Phase 2: Enhanced fields
  entity_type?: string;
  entity_name?: string;
  confidence?: number;
  source_type?: string;
  last_referenced_at?: string;
  // Phase 4: Consolidation
  superseded_by?: number;
  // Phase 7: Temporal
  valid_from?: string;
  valid_until?: string;
  temporal_type?: string;
}

export interface MemoryStats {
  total_count: number;
  by_type: Record<string, number>;
  by_identity: Record<string, number>;
  avg_importance: number;
  oldest_memory_at?: string;
  newest_memory_at?: string;
  superseded_count: number;
  temporal_active_count: number;
}

export interface ListMemoriesParams {
  memory_type?: string;
  identity_id?: string;
  min_importance?: number;
  include_superseded?: boolean;
  limit?: number;
  offset?: number;
}

export async function getMemories(): Promise<MemoryInfo[]> {
  return apiFetch('/memories');
}

export async function getMemoriesFiltered(params: ListMemoriesParams = {}): Promise<MemoryInfo[]> {
  const queryParams = new URLSearchParams();
  if (params.memory_type) queryParams.set('memory_type', params.memory_type);
  if (params.identity_id) queryParams.set('identity_id', params.identity_id);
  if (params.min_importance !== undefined) queryParams.set('min_importance', String(params.min_importance));
  if (params.include_superseded) queryParams.set('include_superseded', 'true');
  if (params.limit) queryParams.set('limit', String(params.limit));
  if (params.offset) queryParams.set('offset', String(params.offset));

  const query = queryParams.toString();
  return apiFetch(`/memories/filtered${query ? `?${query}` : ''}`);
}

export async function getMemory(id: number): Promise<MemoryInfo> {
  return apiFetch(`/memories/${id}`);
}

export async function updateMemory(id: number, data: {
  content?: string;
  category?: string;
  tags?: string;
  importance?: number;
  entity_type?: string;
  entity_name?: string;
  valid_from?: string;
  valid_until?: string;
  temporal_type?: string;
}): Promise<MemoryInfo> {
  return apiFetch(`/memories/${id}`, {
    method: 'PUT',
    body: JSON.stringify(data),
  });
}

export async function deleteMemory(id: string): Promise<void> {
  await apiFetch(`/memories/${id}`, { method: 'DELETE' });
}

export async function mergeMemories(memoryIds: number[], mergedContent: string): Promise<{
  success: boolean;
  merged_memory: MemoryInfo;
  superseded_count: number;
}> {
  return apiFetch('/memories/merge', {
    method: 'POST',
    body: JSON.stringify({
      memory_ids: memoryIds,
      merged_content: mergedContent,
      use_max_importance: true,
    }),
  });
}

export async function getMemoryStats(): Promise<MemoryStats> {
  return apiFetch('/memories/stats');
}

export async function exportMemories(identityId?: string): Promise<string> {
  const query = identityId ? `?identity_id=${encodeURIComponent(identityId)}` : '';
  const response = await fetch(`/api/memories/export${query}`, {
    headers: {
      Authorization: `Bearer ${localStorage.getItem('stark_token')}`,
    },
  });
  if (!response.ok) {
    throw new Error('Failed to export memories');
  }
  return response.text();
}

export async function searchMemories(query: string, params: {
  memory_type?: string;
  identity_id?: string;
  min_importance?: number;
  limit?: number;
} = {}): Promise<Array<{ memory: MemoryInfo; rank: number }>> {
  return apiFetch('/memories/search', {
    method: 'POST',
    body: JSON.stringify({
      query,
      ...params,
      limit: params.limit || 20,
    }),
  });
}

// Identities API
export async function getIdentities(): Promise<Array<{
  id: string;
  name: string;
  channel_type: string;
  platform_user_id: string;
  created_at: string;
}>> {
  return apiFetch('/identities');
}

export interface IdentitySession {
  id: number;
  session_key: string;
  channel_type: string;
  channel_id: number;
  is_active: boolean;
  completion_status: string;
  message_count: number;
  initial_query?: string;
  created_at: string;
  last_activity_at: string;
}

export interface ToolStat {
  tool_name: string;
  total_calls: number;
  successful_calls: number;
}

export interface ToolExecution {
  id: number;
  tool_name: string;
  parameters: Record<string, unknown>;
  success: boolean;
  result?: string;
  duration_ms?: number;
  executed_at: string;
}

export interface LinkedAccount {
  channel_type: string;
  platform_user_id: string;
  platform_user_name?: string;
  is_verified: boolean;
}

export interface IdentityLogs {
  identity_id: string;
  linked_accounts: LinkedAccount[];
  sessions: IdentitySession[];
  session_count: number;
  tool_stats: ToolStat[];
  recent_tool_executions: ToolExecution[];
}

export async function getIdentityLogs(identityId: string): Promise<IdentityLogs> {
  return apiFetch(`/identities/${identityId}/logs`);
}

// Channels API
export interface ChannelInfo {
  id: number;
  channel_type: string;
  name: string;
  enabled: boolean;
  bot_token: string;
  app_token?: string;
  created_at: string;
  updated_at: string;
  running?: boolean;
}

// Channel Settings types
export interface ChannelSetting {
  channel_id: number;
  setting_key: string;
  setting_value: string;
}

export interface ChannelSettingDefinition {
  key: string;
  label: string;
  description: string;
  input_type: 'text' | 'text_area' | 'toggle' | 'number';
  placeholder: string;
}

export interface ChannelSettingsResponse {
  success: boolean;
  settings: ChannelSetting[];
}

export interface ChannelSettingsSchemaResponse {
  success: boolean;
  channel_type: string;
  settings: ChannelSettingDefinition[];
}

interface ChannelsListResponse {
  success: boolean;
  channels?: ChannelInfo[];
  error?: string;
}

interface ChannelOperationResponse {
  success: boolean;
  channel?: ChannelInfo;
  error?: string;
}

export async function getChannels(): Promise<ChannelInfo[]> {
  const response = await apiFetch<ChannelsListResponse>('/channels');
  return response.channels || [];
}

export async function getChannel(id: number): Promise<ChannelInfo | null> {
  const response = await apiFetch<ChannelOperationResponse>(`/channels/${id}`);
  return response.channel || null;
}

export async function createChannel(data: {
  channel_type: string;
  name: string;
  bot_token: string;
  app_token?: string;
}): Promise<ChannelInfo> {
  const response = await apiFetch<ChannelOperationResponse>('/channels', {
    method: 'POST',
    body: JSON.stringify(data),
  });
  if (!response.success || !response.channel) {
    throw new Error(response.error || 'Failed to create channel');
  }
  return response.channel;
}

export async function updateChannel(id: number, config: {
  name?: string;
  enabled?: boolean;
  bot_token?: string;
  app_token?: string;
}): Promise<ChannelInfo> {
  const response = await apiFetch<ChannelOperationResponse>(`/channels/${id}`, {
    method: 'PUT',
    body: JSON.stringify(config),
  });
  if (!response.success || !response.channel) {
    throw new Error(response.error || 'Failed to update channel');
  }
  return response.channel;
}

export async function deleteChannel(id: number): Promise<void> {
  const response = await apiFetch<ChannelOperationResponse>(`/channels/${id}`, {
    method: 'DELETE',
  });
  if (!response.success) {
    throw new Error(response.error || 'Failed to delete channel');
  }
}

export async function startChannel(id: number): Promise<ChannelInfo> {
  const response = await apiFetch<ChannelOperationResponse>(`/channels/${id}/start`, {
    method: 'POST',
  });
  if (!response.success || !response.channel) {
    throw new Error(response.error || 'Failed to start channel');
  }
  return response.channel;
}

export async function stopChannel(id: number): Promise<ChannelInfo> {
  const response = await apiFetch<ChannelOperationResponse>(`/channels/${id}/stop`, {
    method: 'POST',
  });
  if (!response.success || !response.channel) {
    throw new Error(response.error || 'Failed to stop channel');
  }
  return response.channel;
}

// Channel Settings API
export async function getChannelSettingsSchema(channelType: string): Promise<ChannelSettingDefinition[]> {
  const response = await apiFetch<ChannelSettingsSchemaResponse>(`/channels/settings/schema/${channelType}`);
  return response.settings || [];
}

export async function getChannelSettings(channelId: number): Promise<ChannelSetting[]> {
  const response = await apiFetch<ChannelSettingsResponse>(`/channels/${channelId}/settings`);
  return response.settings || [];
}

export async function updateChannelSettings(
  channelId: number,
  settings: Array<{ key: string; value: string }>
): Promise<ChannelSetting[]> {
  const response = await apiFetch<ChannelSettingsResponse>(`/channels/${channelId}/settings`, {
    method: 'PUT',
    body: JSON.stringify({ settings }),
  });
  return response.settings || [];
}

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

// Cloud Backup API
export interface BackupResponse {
  success: boolean;
  key_count?: number;
  node_count?: number;
  connection_count?: number;
  cron_job_count?: number;
  channel_count?: number;
  channel_setting_count?: number;
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

// Bot Settings API
export interface BotSettings {
  id: number;
  bot_name: string;
  bot_email: string;
  web3_tx_requires_confirmation: boolean;
  rpc_provider: string;
  custom_rpc_endpoints?: Record<string, string>;
  max_tool_iterations: number;
  rogue_mode_enabled: boolean;
  safe_mode_max_queries_per_10min: number;
  keystore_url?: string;
  created_at: string;
  updated_at: string;
}

export async function getBotSettings(): Promise<BotSettings> {
  return apiFetch('/bot-settings');
}

export async function updateBotSettings(data: {
  bot_name?: string;
  bot_email?: string;
  web3_tx_requires_confirmation?: boolean;
  rpc_provider?: string;
  custom_rpc_endpoints?: Record<string, string>;
  max_tool_iterations?: number;
  rogue_mode_enabled?: boolean;
  safe_mode_max_queries_per_10min?: number;
  keystore_url?: string;
}): Promise<BotSettings> {
  return apiFetch('/bot-settings', {
    method: 'PUT',
    body: JSON.stringify(data),
  });
}

// RPC Providers API
export interface RpcProvider {
  id: string;
  display_name: string;
  description: string;
  x402: boolean;
  networks: string[];
}

export async function getRpcProviders(): Promise<RpcProvider[]> {
  return apiFetch('/rpc-providers');
}

// Auto-sync status API
export interface AutoSyncStatus {
  status: string | null;
  message: string;
  synced_at?: string;
  key_count?: number;
  node_count?: number;
  keystore_url: string;
}

export async function getAutoSyncStatus(): Promise<AutoSyncStatus> {
  return apiFetch('/auto-sync-status');
}

// Confirmation API
export interface ConfirmationResponse {
  success: boolean;
  message?: string;
  error?: string;
  result?: string;
}

export interface PendingConfirmationResponse {
  has_pending: boolean;
  confirmation?: {
    id: string;
    channel_id: number;
    tool_name: string;
    description: string;
    parameters: Record<string, unknown>;
  };
}

export async function getPendingConfirmation(channelId: number): Promise<PendingConfirmationResponse> {
  return apiFetch(`/confirmation/pending/${channelId}`);
}

export async function confirmTransaction(channelId: number): Promise<ConfirmationResponse> {
  return apiFetch('/confirmation/confirm', {
    method: 'POST',
    body: JSON.stringify({ channel_id: channelId }),
  });
}

export async function cancelTransaction(channelId: number): Promise<ConfirmationResponse> {
  return apiFetch('/confirmation/cancel', {
    method: 'POST',
    body: JSON.stringify({ channel_id: channelId }),
  });
}

// Execution Control API
export interface StopExecutionResponse {
  success: boolean;
  message?: string;
  error?: string;
}

export async function stopExecution(): Promise<StopExecutionResponse> {
  return apiFetch('/chat/stop', {
    method: 'POST',
  });
}

// Execution Status API
export interface ExecutionStatusResponse {
  running: boolean;
  execution_id: string | null;
}

export async function getExecutionStatus(): Promise<ExecutionStatusResponse> {
  return apiFetch('/chat/execution-status');
}

// Task Queue API
export interface PlannerTaskInfo {
  id: number;
  description: string;
  status: string;
}

export interface GetPlannerTasksResponse {
  success: boolean;
  tasks: PlannerTaskInfo[];
}

export interface DeleteTaskResponse {
  success: boolean;
  message?: string;
  error?: string;
  was_current_task?: boolean;
}

export async function getPlannerTasks(): Promise<GetPlannerTasksResponse> {
  return apiFetch('/chat/tasks');
}

export async function deletePlannerTask(taskId: number): Promise<DeleteTaskResponse> {
  return apiFetch(`/chat/tasks/${taskId}`, { method: 'DELETE' });
}

// Subagent API
// Types imported from shared subagent-types.ts which matches Rust SubAgentStatus enum
import { Subagent, SubagentStatus } from '@/lib/subagent-types';
export { SubagentStatus };
export type SubagentInfo = Subagent;

export interface SubagentListResponse {
  success: boolean;
  subagents: SubagentInfo[];
}

export interface SubagentResponse {
  success: boolean;
  message?: string;
  error?: string;
}

export async function listSubagents(): Promise<SubagentListResponse> {
  return apiFetch('/chat/subagents');
}

export async function cancelSubagent(subagentId: string): Promise<SubagentResponse> {
  return apiFetch('/chat/subagents/cancel', {
    method: 'POST',
    body: JSON.stringify({ subagent_id: subagentId }),
  });
}

// Files API
export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified?: string;
}

export interface ListFilesResponse {
  success: boolean;
  path: string;
  entries: FileEntry[];
  error?: string;
}

export interface ReadFileResponse {
  success: boolean;
  path: string;
  content?: string;
  size?: number;
  is_binary?: boolean;
  error?: string;
}

export interface WorkspaceInfoResponse {
  success: boolean;
  workspace_path: string;
  exists: boolean;
}

export async function listFiles(path?: string): Promise<ListFilesResponse> {
  const query = path ? `?path=${encodeURIComponent(path)}` : '';
  return apiFetch(`/files${query}`);
}

export async function readFile(path: string): Promise<ReadFileResponse> {
  return apiFetch(`/files/read?path=${encodeURIComponent(path)}`);
}

export async function getWorkspaceInfo(): Promise<WorkspaceInfoResponse> {
  return apiFetch('/files/workspace');
}

// Session Transcript API
export interface SessionMessage {
  id: number;
  session_id: number;
  role: string;
  content: string;
  created_at: string;
}

export interface SessionTranscriptResponse {
  session_id: number;
  messages: SessionMessage[];
  total_count: number;
}

export async function getSessionTranscript(sessionId: number, limit?: number): Promise<SessionTranscriptResponse> {
  const query = limit ? `?limit=${limit}` : '';
  return apiFetch(`/sessions/${sessionId}/transcript${query}`);
}

// Intrinsic Files API
export interface IntrinsicFileInfo {
  name: string;
  description: string;
  writable: boolean;
}

export interface IntrinsicFileContent {
  success: boolean;
  name: string;
  content?: string;
  writable: boolean;
  error?: string;
}

interface ListIntrinsicResponse {
  success: boolean;
  files: IntrinsicFileInfo[];
}

interface WriteIntrinsicResponse {
  success: boolean;
  error?: string;
}

export async function listIntrinsicFiles(): Promise<IntrinsicFileInfo[]> {
  const response = await apiFetch<ListIntrinsicResponse>('/intrinsic');
  return response.files || [];
}

export async function readIntrinsicFile(name: string): Promise<IntrinsicFileContent> {
  return apiFetch(`/intrinsic/${encodeURIComponent(name)}`);
}

export async function writeIntrinsicFile(name: string, content: string): Promise<WriteIntrinsicResponse> {
  return apiFetch(`/intrinsic/${encodeURIComponent(name)}`, {
    method: 'PUT',
    body: JSON.stringify({ content }),
  });
}

// Journal API
export interface JournalEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified?: string;
}

export interface ListJournalResponse {
  success: boolean;
  path: string;
  entries: JournalEntry[];
  error?: string;
}

export interface ReadJournalResponse {
  success: boolean;
  path: string;
  content?: string;
  size?: number;
  error?: string;
}

export interface JournalInfoResponse {
  success: boolean;
  journal_path: string;
  exists: boolean;
}

export async function listJournal(path?: string): Promise<ListJournalResponse> {
  const query = path ? `?path=${encodeURIComponent(path)}` : '';
  return apiFetch(`/journal${query}`);
}

export async function readJournalFile(path: string): Promise<ReadJournalResponse> {
  return apiFetch(`/journal/read?path=${encodeURIComponent(path)}`);
}

export async function getJournalInfo(): Promise<JournalInfoResponse> {
  return apiFetch('/journal/info');
}

// Transaction Queue API
export interface QueuedTransactionInfo {
  uuid: string;
  network: string;
  from: string;
  to: string;
  value: string;
  value_formatted: string;
  /** Hex-encoded calldata for function selector lookup */
  data: string;
  status: 'pending' | 'broadcasting' | 'broadcast' | 'confirmed' | 'failed' | 'expired';
  tx_hash?: string;
  explorer_url?: string;
  error?: string;
  created_at: string;
  broadcast_at?: string;
}

export interface QueuedTransactionsResponse {
  success: boolean;
  transactions: QueuedTransactionInfo[];
  total: number;
  pending_count: number;
  confirmed_count: number;
  failed_count: number;
}

export interface QueuedTransactionResponse {
  success: boolean;
  transaction?: QueuedTransactionInfo;
  error?: string;
}

export async function getQueuedTransactions(status?: string, limit?: number): Promise<QueuedTransactionsResponse> {
  const params = new URLSearchParams();
  if (status) params.set('status', status);
  if (limit) params.set('limit', String(limit));
  const query = params.toString();
  return apiFetch(`/tx-queue${query ? `?${query}` : ''}`);
}

export async function getPendingTransactions(): Promise<QueuedTransactionsResponse> {
  return apiFetch('/tx-queue/pending');
}

export async function getQueuedTransaction(uuid: string): Promise<QueuedTransactionResponse> {
  return apiFetch(`/tx-queue/${encodeURIComponent(uuid)}`);
}

// Broadcasted Transactions API (persistent history)
export interface BroadcastedTransactionInfo {
  id: number;
  uuid: string;
  network: string;
  from_address: string;
  to_address: string;
  value: string;
  value_formatted: string;
  tx_hash?: string;
  explorer_url?: string;
  status: 'broadcast' | 'confirmed' | 'failed';
  broadcast_mode: 'rogue' | 'partner';
  error?: string;
  broadcast_at: string;
  confirmed_at?: string;
  created_at: string;
}

export interface BroadcastedTransactionsResponse {
  success: boolean;
  transactions: BroadcastedTransactionInfo[];
  total: number;
}

export async function getBroadcastedTransactions(params?: {
  status?: string;
  network?: string;
  broadcast_mode?: string;
  limit?: number;
}): Promise<BroadcastedTransactionsResponse> {
  const queryParams = new URLSearchParams();
  if (params?.status) queryParams.set('status', params.status);
  if (params?.network) queryParams.set('network', params.network);
  if (params?.broadcast_mode) queryParams.set('broadcast_mode', params.broadcast_mode);
  if (params?.limit) queryParams.set('limit', String(params.limit));
  const query = queryParams.toString();
  return apiFetch(`/broadcasted-transactions${query ? `?${query}` : ''}`);
}

// Mind Map API
export interface MindNodeInfo {
  id: number;
  body: string;
  position_x: number | null;
  position_y: number | null;
  is_trunk: boolean;
  created_at: string;
  updated_at: string;
}

export interface MindConnectionInfo {
  id: number;
  parent_id: number;
  child_id: number;
  created_at: string;
}

export interface MindGraphResponse {
  nodes: MindNodeInfo[];
  connections: MindConnectionInfo[];
}

export async function getMindGraph(): Promise<MindGraphResponse> {
  return apiFetch('/mindmap/graph');
}

export async function getMindNodes(): Promise<MindNodeInfo[]> {
  return apiFetch('/mindmap/nodes');
}

export async function createMindNode(data: {
  body?: string;
  position_x?: number;
  position_y?: number;
  parent_id?: number;
}): Promise<MindNodeInfo> {
  return apiFetch('/mindmap/nodes', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function updateMindNode(id: number, data: {
  body?: string;
  position_x?: number;
  position_y?: number;
}): Promise<MindNodeInfo> {
  return apiFetch(`/mindmap/nodes/${id}`, {
    method: 'PUT',
    body: JSON.stringify(data),
  });
}

export async function deleteMindNode(id: number): Promise<{ success: boolean; message: string }> {
  return apiFetch(`/mindmap/nodes/${id}`, {
    method: 'DELETE',
  });
}

export async function getMindConnections(): Promise<MindConnectionInfo[]> {
  return apiFetch('/mindmap/connections');
}

export async function createMindConnection(parentId: number, childId: number): Promise<MindConnectionInfo> {
  return apiFetch('/mindmap/connections', {
    method: 'POST',
    body: JSON.stringify({ parent_id: parentId, child_id: childId }),
  });
}

export async function deleteMindConnection(parentId: number, childId: number): Promise<{ success: boolean; message: string }> {
  return apiFetch(`/mindmap/connections/${parentId}/${childId}`, {
    method: 'DELETE',
  });
}

// Heartbeat session info for mind map sidebar
export interface HeartbeatSessionInfo {
  id: number;
  mind_node_id: number | null;
  created_at: string;
  message_count: number;
}

export async function getHeartbeatSessions(): Promise<HeartbeatSessionInfo[]> {
  return apiFetch('/mindmap/heartbeat-sessions');
}

// Guest Mind Map API (no auth required)
export async function getGuestMindGraph(): Promise<MindGraphResponse> {
  const response = await fetch(`${API_BASE}/mindmap/graph/guest`);
  if (!response.ok) {
    if (response.status === 403) {
      throw new Error('Guest dashboard is not enabled');
    }
    throw new Error('Failed to fetch guest mind graph');
  }
  return response.json();
}
