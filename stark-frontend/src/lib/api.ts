const API_BASE = '/api';

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
  conversationHistory: Array<{ role: string; content: string }>
): Promise<{ response: string }> {
  // Backend expects { messages: [...] } with the full conversation including the new message
  const messages = [
    ...conversationHistory,
    { role: 'user', content }
  ];

  const response = await apiFetch<{ success: boolean; message?: { content: string }; error?: string }>('/chat', {
    method: 'POST',
    body: JSON.stringify({ messages }),
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
  created_at: string;
  updated_at: string;
  message_count?: number;
}>> {
  return apiFetch('/sessions');
}

export async function deleteSession(id: string): Promise<void> {
  await apiFetch(`/sessions/${id}`, { method: 'DELETE' });
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

// Bot Settings API
export interface BotSettings {
  id: number;
  bot_name: string;
  bot_email: string;
  web3_tx_requires_confirmation: boolean;
  rpc_provider: string;
  custom_rpc_endpoints?: Record<string, string>;
  max_tool_iterations: number;
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

// Subagent API
export interface SubagentInfo {
  id: string;
  label: string;
  task: string;
  status: string;
  started_at: string;
}

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
