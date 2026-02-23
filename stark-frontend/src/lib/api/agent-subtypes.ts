import { apiFetch } from './core';

// Agent Subtypes API
export interface AgentSubtypeInfo {
  key: string;
  label: string;
  emoji: string;
  description: string;
  tool_groups: string[];
  skill_tags: string[];
  additional_tools: string[];
  prompt: string;
  sort_order: number;
  enabled: boolean;
  max_iterations: number;
  skip_task_planner: boolean;
  hidden: boolean;
  aliases?: string[];
  preferred_ai_model?: string | null;
  hooks?: { event: string }[];
}

export async function getAgentSubtypes(): Promise<AgentSubtypeInfo[]> {
  return apiFetch('/agent-subtypes');
}

export async function getAgentSubtype(key: string): Promise<AgentSubtypeInfo> {
  return apiFetch(`/agent-subtypes/${encodeURIComponent(key)}`);
}

export async function createAgentSubtype(config: AgentSubtypeInfo): Promise<AgentSubtypeInfo> {
  return apiFetch('/agent-subtypes', {
    method: 'POST',
    body: JSON.stringify(config),
  });
}

export async function updateAgentSubtype(key: string, config: Partial<AgentSubtypeInfo>): Promise<AgentSubtypeInfo> {
  return apiFetch(`/agent-subtypes/${encodeURIComponent(key)}`, {
    method: 'PUT',
    body: JSON.stringify(config),
  });
}

export async function deleteAgentSubtype(key: string): Promise<{ success: boolean; message: string }> {
  return apiFetch(`/agent-subtypes/${encodeURIComponent(key)}`, {
    method: 'DELETE',
  });
}

export async function resetAgentSubtypeDefaults(): Promise<{ success: boolean; message: string; count: number }> {
  return apiFetch('/agent-subtypes/reset-defaults', {
    method: 'POST',
  });
}

export async function exportAgentSubtypes(): Promise<string> {
  const token = localStorage.getItem('stark_token');
  const response = await fetch('/api/agent-subtypes/export', {
    headers: token ? { Authorization: `Bearer ${token}` } : {},
  });
  if (!response.ok) throw new Error('Failed to export agent subtypes');
  return response.text();
}

export async function exportAgentSubtype(key: string): Promise<string> {
  const token = localStorage.getItem('stark_token');
  const response = await fetch(`/api/agent-subtypes/${encodeURIComponent(key)}/export`, {
    headers: token ? { Authorization: `Bearer ${token}` } : {},
  });
  if (!response.ok) throw new Error('Failed to export agent subtype');
  return response.text();
}

// StarkHub integration
export interface FeaturedAgentSubtype {
  id: string;
  slug: string;
  key: string;
  label: string;
  emoji: string;
  description: string;
  version: string;
  author_username: string | null;
  author_address: string;
  install_count: number;
  status: string;
}

export async function getFeaturedAgentSubtypes(): Promise<FeaturedAgentSubtype[]> {
  return apiFetch('/agent-subtypes/featured_remote');
}

export async function installAgentSubtypeFromHub(
  username: string,
  slug: string
): Promise<{ success: boolean; key: string; label: string; files: string[]; message: string }> {
  return apiFetch('/agent-subtypes/install', {
    method: 'POST',
    body: JSON.stringify({ username, slug }),
  });
}

export async function publishAgentSubtype(
  key: string,
  starkHubToken: string
): Promise<{ success: boolean; slug: string; username: string; message: string }> {
  const token = localStorage.getItem('stark_token');
  const response = await fetch(`/api/agent-subtypes/publish/${encodeURIComponent(key)}`, {
    method: 'POST',
    headers: {
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      'X-StarkHub-Token': starkHubToken,
    },
  });
  if (!response.ok) {
    const body = await response.json().catch(() => ({ error: 'Unknown error' }));
    throw new Error(body.error || 'Failed to publish');
  }
  return response.json();
}

export async function importAgentSubtypes(ron: string, replace: boolean): Promise<{
  success: boolean;
  imported: number;
  total: number;
  message: string;
  errors?: string[];
}> {
  return apiFetch('/agent-subtypes/import', {
    method: 'POST',
    body: JSON.stringify({ ron, replace }),
  });
}
