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
  aliases?: string[];
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
