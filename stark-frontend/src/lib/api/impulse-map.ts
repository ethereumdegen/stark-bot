import { API_BASE, apiFetch } from './core';

// Impulse Map API
export interface ImpulseNodeInfo {
  id: number;
  body: string;
  position_x: number | null;
  position_y: number | null;
  is_trunk: boolean;
  created_at: string;
  updated_at: string;
}

export interface ImpulseConnectionInfo {
  id: number;
  parent_id: number;
  child_id: number;
  created_at: string;
}

export interface ImpulseGraphResponse {
  nodes: ImpulseNodeInfo[];
  connections: ImpulseConnectionInfo[];
}

export async function getImpulseGraph(): Promise<ImpulseGraphResponse> {
  return apiFetch('/impulse-map/graph');
}

export async function getImpulseNodes(): Promise<ImpulseNodeInfo[]> {
  return apiFetch('/impulse-map/nodes');
}

export async function createImpulseNode(data: {
  body?: string;
  position_x?: number;
  position_y?: number;
  parent_id?: number;
}): Promise<ImpulseNodeInfo> {
  return apiFetch('/impulse-map/nodes', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function updateImpulseNode(id: number, data: {
  body?: string;
  position_x?: number;
  position_y?: number;
}): Promise<ImpulseNodeInfo> {
  return apiFetch(`/impulse-map/nodes/${id}`, {
    method: 'PUT',
    body: JSON.stringify(data),
  });
}

export async function deleteImpulseNode(id: number): Promise<{ success: boolean; message: string }> {
  return apiFetch(`/impulse-map/nodes/${id}`, {
    method: 'DELETE',
  });
}

export async function getImpulseConnections(): Promise<ImpulseConnectionInfo[]> {
  return apiFetch('/impulse-map/connections');
}

export async function createImpulseConnection(parentId: number, childId: number): Promise<ImpulseConnectionInfo> {
  return apiFetch('/impulse-map/connections', {
    method: 'POST',
    body: JSON.stringify({ parent_id: parentId, child_id: childId }),
  });
}

export async function deleteImpulseConnection(parentId: number, childId: number): Promise<{ success: boolean; message: string }> {
  return apiFetch(`/impulse-map/connections/${parentId}/${childId}`, {
    method: 'DELETE',
  });
}

// Heartbeat session info for impulse map sidebar
export interface ImpulseHeartbeatSessionInfo {
  id: number;
  impulse_node_id: number | null;
  created_at: string;
  message_count: number;
}

export async function getImpulseHeartbeatSessions(): Promise<ImpulseHeartbeatSessionInfo[]> {
  return apiFetch('/impulse-map/heartbeat-sessions');
}

// Guest Impulse Map API (no auth required)
export async function getGuestImpulseGraph(): Promise<ImpulseGraphResponse> {
  const response = await fetch(`${API_BASE}/impulse-map/graph/guest`);
  if (!response.ok) {
    if (response.status === 403) {
      throw new Error('Guest dashboard is not enabled');
    }
    throw new Error('Failed to fetch guest impulse graph');
  }
  return response.json();
}
