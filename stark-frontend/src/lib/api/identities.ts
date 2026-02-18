import { apiFetch } from './core';

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
