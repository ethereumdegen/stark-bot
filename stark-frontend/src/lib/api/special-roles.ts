import { apiFetch } from './core';

// Special Roles API (enriched safe mode)

export interface SpecialRoleInfo {
  name: string;
  allowed_tools: string[];
  allowed_skills: string[];
  description: string | null;
  created_at: string;
  updated_at: string;
}

export interface SpecialRoleAssignmentInfo {
  id: number;
  channel_type: string;
  user_id: string;
  special_role_name: string;
  label: string | null;
  created_at: string;
}

export async function getSpecialRoles(): Promise<SpecialRoleInfo[]> {
  return apiFetch('/special-roles');
}

export async function getSpecialRole(name: string): Promise<SpecialRoleInfo> {
  return apiFetch(`/special-roles/${encodeURIComponent(name)}`);
}

export async function createSpecialRole(role: {
  name: string;
  allowed_tools: string[];
  allowed_skills: string[];
  description?: string;
}): Promise<SpecialRoleInfo> {
  return apiFetch('/special-roles', {
    method: 'POST',
    body: JSON.stringify(role),
  });
}

export async function updateSpecialRole(
  name: string,
  update: {
    allowed_tools?: string[];
    allowed_skills?: string[];
    description?: string | null;
  }
): Promise<SpecialRoleInfo> {
  return apiFetch(`/special-roles/${encodeURIComponent(name)}`, {
    method: 'PUT',
    body: JSON.stringify(update),
  });
}

export async function deleteSpecialRole(name: string): Promise<{ success: boolean; message: string }> {
  return apiFetch(`/special-roles/${encodeURIComponent(name)}`, {
    method: 'DELETE',
  });
}

export async function getSpecialRoleAssignments(roleName?: string): Promise<SpecialRoleAssignmentInfo[]> {
  const params = roleName ? `?role_name=${encodeURIComponent(roleName)}` : '';
  return apiFetch(`/special-roles/assignments${params}`);
}

export async function createSpecialRoleAssignment(assignment: {
  channel_type: string;
  user_id: string;
  special_role_name: string;
  label?: string;
}): Promise<SpecialRoleAssignmentInfo> {
  return apiFetch('/special-roles/assignments', {
    method: 'POST',
    body: JSON.stringify(assignment),
  });
}

export async function deleteSpecialRoleAssignment(id: number): Promise<{ success: boolean; message: string }> {
  return apiFetch(`/special-roles/assignments/${id}`, {
    method: 'DELETE',
  });
}
