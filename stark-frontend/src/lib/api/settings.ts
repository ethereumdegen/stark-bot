import { apiFetch } from './core';

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
  chat_session_memory_generation: boolean;
  guest_dashboard_enabled: boolean;
  theme_accent?: string;
  proxy_url?: string;
  kanban_auto_execute: boolean;
  coalescing_enabled: boolean;
  coalescing_debounce_ms: number;
  coalescing_max_wait_ms: number;
  compaction_background_threshold: number;
  compaction_aggressive_threshold: number;
  compaction_emergency_threshold: number;
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
  chat_session_memory_generation?: boolean;
  coalescing_enabled?: boolean;
  coalescing_debounce_ms?: number;
  coalescing_max_wait_ms?: number;
  compaction_background_threshold?: number;
  compaction_aggressive_threshold?: number;
  compaction_emergency_threshold?: number;
  guest_dashboard_enabled?: boolean;
  theme_accent?: string;
  proxy_url?: string;
  kanban_auto_execute?: boolean;
}): Promise<BotSettings> {
  return apiFetch('/bot-settings', {
    method: 'PUT',
    body: JSON.stringify(data),
  });
}

// AI Endpoint Presets API
export interface AiEndpointPreset {
  id: string;
  display_name: string;
  endpoint: string;
  model_archetype: string;
  x402_cost: number | null;
}

export async function getAiEndpointPresets(): Promise<AiEndpointPreset[]> {
  return apiFetch('/agent-settings/endpoints');
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
