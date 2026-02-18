import { API_BASE, apiFetch } from './core';

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
