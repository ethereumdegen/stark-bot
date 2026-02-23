import { API_BASE, apiFetch } from './core';

// Notes API
export interface NoteEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified?: string;
}

export interface ListNotesResponse {
  success: boolean;
  path: string;
  entries: NoteEntry[];
  error?: string;
}

export interface ReadNoteResponse {
  success: boolean;
  path: string;
  content?: string;
  size?: number;
  title?: string;
  tags?: string[];
  note_type?: string;
  error?: string;
}

export interface NotesInfoResponse {
  success: boolean;
  notes_path: string;
  exists: boolean;
  file_count: number;
}

export interface SearchNotesResponse {
  success: boolean;
  query: string;
  results: SearchResultItem[];
  error?: string;
}

export interface SearchResultItem {
  file_path: string;
  title: string;
  tags: string;
  snippet: string;
}

export interface TagItem {
  tag: string;
  count: number;
}

export interface TagsResponse {
  success: boolean;
  tags: TagItem[];
  error?: string;
}

export interface NotesByTagGroup {
  tag: string;
  count: number;
  notes: { file_path: string; title: string; tags: string }[];
}

export interface NotesByTagResponse {
  success: boolean;
  groups: NotesByTagGroup[];
  error?: string;
}

export async function listNotes(path?: string): Promise<ListNotesResponse> {
  const query = path ? `?path=${encodeURIComponent(path)}` : '';
  return apiFetch(`/notes${query}`);
}

export async function readNoteFile(path: string): Promise<ReadNoteResponse> {
  return apiFetch(`/notes/read?path=${encodeURIComponent(path)}`);
}

export async function searchNotes(q: string, limit?: number): Promise<SearchNotesResponse> {
  const params = new URLSearchParams({ q });
  if (limit) params.set('limit', String(limit));
  return apiFetch(`/notes/search?${params.toString()}`);
}

export async function getNotesInfo(): Promise<NotesInfoResponse> {
  return apiFetch('/notes/info');
}

export async function getNotesTags(): Promise<TagsResponse> {
  return apiFetch('/notes/tags');
}

export async function getNotesByTag(): Promise<NotesByTagResponse> {
  return apiFetch('/notes/by-tag');
}

export async function exportNotesZip(): Promise<Blob> {
  const token = localStorage.getItem('stark_token');
  const headers: HeadersInit = {};
  if (token) headers['Authorization'] = `Bearer ${token}`;

  const response = await fetch(`${API_BASE}/notes/export`, { headers });
  if (!response.ok) {
    const data = await response.json().catch(() => ({ error: `HTTP ${response.status}` }));
    throw new Error(data.error || 'Export failed');
  }
  return response.blob();
}

export async function deleteNote(path: string): Promise<{ success: boolean; error?: string }> {
  return apiFetch(`/notes/delete?path=${encodeURIComponent(path)}`, { method: 'DELETE' });
}
