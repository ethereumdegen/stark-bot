import { apiFetch } from './core';

// Files API
export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified?: string;
}

export interface ListFilesResponse {
  success: boolean;
  path: string;
  entries: FileEntry[];
  error?: string;
}

export interface ReadFileResponse {
  success: boolean;
  path: string;
  content?: string;
  size?: number;
  is_binary?: boolean;
  error?: string;
}

export interface WorkspaceInfoResponse {
  success: boolean;
  workspace_path: string;
  exists: boolean;
}

export async function listFiles(path?: string): Promise<ListFilesResponse> {
  const query = path ? `?path=${encodeURIComponent(path)}` : '';
  return apiFetch(`/files${query}`);
}

export async function readFile(path: string): Promise<ReadFileResponse> {
  return apiFetch(`/files/read?path=${encodeURIComponent(path)}`);
}

export async function getWorkspaceInfo(): Promise<WorkspaceInfoResponse> {
  return apiFetch('/files/workspace');
}

export async function listFilesWithSizes(path?: string): Promise<ListFilesResponse> {
  const params = new URLSearchParams();
  if (path) params.set('path', path);
  params.set('include_dir_sizes', 'true');
  return apiFetch(`/files?${params.toString()}`);
}

export async function deleteWorkspaceFile(path: string): Promise<{ success: boolean; deleted_count: number; freed_bytes: number; error?: string }> {
  return apiFetch('/files/delete', {
    method: 'DELETE',
    body: JSON.stringify({ path }),
  });
}

// Intrinsic Files API
export interface IntrinsicFileInfo {
  name: string;
  description: string;
  writable: boolean;
  deletable?: boolean;
}

export interface IntrinsicFileContent {
  success: boolean;
  name: string;
  content?: string;
  writable: boolean;
  error?: string;
}

interface ListIntrinsicResponse {
  success: boolean;
  files: IntrinsicFileInfo[];
}

interface WriteIntrinsicResponse {
  success: boolean;
  error?: string;
}

export async function listIntrinsicFiles(): Promise<IntrinsicFileInfo[]> {
  const response = await apiFetch<ListIntrinsicResponse>('/intrinsic');
  return response.files || [];
}

export async function readIntrinsicFile(name: string): Promise<IntrinsicFileContent> {
  return apiFetch(`/intrinsic/${encodeURIComponent(name)}`);
}

export async function writeIntrinsicFile(name: string, content: string): Promise<WriteIntrinsicResponse> {
  return apiFetch(`/intrinsic/${encodeURIComponent(name)}`, {
    method: 'PUT',
    body: JSON.stringify({ content }),
  });
}

export async function deleteIntrinsicFile(name: string): Promise<WriteIntrinsicResponse> {
  return apiFetch(`/intrinsic/${encodeURIComponent(name)}`, {
    method: 'DELETE',
  });
}

// Journal API
export interface JournalEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified?: string;
}

export interface ListJournalResponse {
  success: boolean;
  path: string;
  entries: JournalEntry[];
  error?: string;
}

export interface ReadJournalResponse {
  success: boolean;
  path: string;
  content?: string;
  size?: number;
  error?: string;
}

export interface JournalInfoResponse {
  success: boolean;
  journal_path: string;
  exists: boolean;
}

export async function listJournal(path?: string): Promise<ListJournalResponse> {
  const query = path ? `?path=${encodeURIComponent(path)}` : '';
  return apiFetch(`/journal${query}`);
}

export async function readJournalFile(path: string): Promise<ReadJournalResponse> {
  return apiFetch(`/journal/read?path=${encodeURIComponent(path)}`);
}

export async function getJournalInfo(): Promise<JournalInfoResponse> {
  return apiFetch('/journal/info');
}
