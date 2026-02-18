import { API_BASE, apiFetch } from './core';

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

export interface SkillDetail {
  name: string;
  description: string;
  version: string;
  source: string;
  path: string;
  enabled: boolean;
  requires_tools: string[];
  requires_binaries: string[];
  missing_binaries: string[];
  tags: string[];
  arguments: Array<{ name: string; description: string; required: boolean; default?: string }>;
  prompt_template: string;
  scripts?: Array<{ name: string; language: string }>;
  homepage?: string;
  metadata?: string;
}

export interface SkillDetailResponse {
  success: boolean;
  skill?: SkillDetail;
  error?: string;
}

export async function getSkills(): Promise<SkillInfo[]> {
  return apiFetch('/skills');
}

export async function getSkillDetail(name: string): Promise<SkillDetail> {
  const response = await apiFetch<SkillDetailResponse>(`/skills/${encodeURIComponent(name)}`);
  if (!response.success || !response.skill) {
    throw new Error(response.error || 'Failed to get skill detail');
  }
  return response.skill;
}

export async function updateSkillBody(name: string, body: string): Promise<void> {
  await apiFetch(`/skills/${encodeURIComponent(name)}`, {
    method: 'PUT',
    body: JSON.stringify({ body }),
  });
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
