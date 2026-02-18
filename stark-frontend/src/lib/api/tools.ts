import { apiFetch } from './core';

// Tools API
export interface ToolInfo {
  name: string;
  description: string;
  group: string;
  enabled: boolean;
  safety_level: string;
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
