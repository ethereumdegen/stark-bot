import { apiFetch } from './core';

// Kanban Board API
export interface KanbanItem {
  id: number;
  title: string;
  description: string;
  status: 'ready' | 'in_progress' | 'complete';
  priority: number;
  session_id: number | null;
  result: string | null;
  created_at: string;
  updated_at: string;
}

export async function getKanbanItems(status?: string): Promise<KanbanItem[]> {
  const params = status ? `?status=${encodeURIComponent(status)}` : '';
  return apiFetch(`/kanban/items${params}`);
}

export async function createKanbanItem(data: {
  title: string;
  description?: string;
  priority?: number;
}): Promise<KanbanItem> {
  return apiFetch('/kanban/items', {
    method: 'POST',
    body: JSON.stringify(data),
  });
}

export async function updateKanbanItem(id: number, data: {
  title?: string;
  description?: string;
  status?: string;
  priority?: number;
  session_id?: number;
  result?: string;
}): Promise<KanbanItem> {
  return apiFetch(`/kanban/items/${id}`, {
    method: 'PUT',
    body: JSON.stringify(data),
  });
}

export async function deleteKanbanItem(id: number): Promise<{ success: boolean; message: string }> {
  return apiFetch(`/kanban/items/${id}`, {
    method: 'DELETE',
  });
}
