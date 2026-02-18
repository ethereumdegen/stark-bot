import { apiFetch } from './core';
import type {
  MemoryGraphResponse,
  HybridSearchResponse,
  EmbeddingStatsResponse,
  MemoryAssociation,
  CortexBulletin,
} from '@/types';

// Memories API - Enhanced (Phase 5)
export interface MemoryInfo {
  id: number;
  memory_type: string;
  content: string;
  category?: string;
  tags?: string;
  importance: number;
  identity_id?: string;
  source_channel_type?: string;
  log_date?: string;
  created_at: string;
  updated_at: string;
  // Phase 2: Enhanced fields
  entity_type?: string;
  entity_name?: string;
  confidence?: number;
  source_type?: string;
  last_referenced_at?: string;
  // Phase 4: Consolidation
  superseded_by?: number;
  // Phase 7: Temporal
  valid_from?: string;
  valid_until?: string;
  temporal_type?: string;
}

export interface MemoryStats {
  total_count: number;
  by_type: Record<string, number>;
  by_identity: Record<string, number>;
  avg_importance: number;
  oldest_memory_at?: string;
  newest_memory_at?: string;
  superseded_count: number;
  temporal_active_count: number;
}

export interface ListMemoriesParams {
  memory_type?: string;
  identity_id?: string;
  min_importance?: number;
  include_superseded?: boolean;
  limit?: number;
  offset?: number;
}

export async function getMemories(): Promise<MemoryInfo[]> {
  return apiFetch('/memories');
}

export async function getMemoriesFiltered(params: ListMemoriesParams = {}): Promise<MemoryInfo[]> {
  const queryParams = new URLSearchParams();
  if (params.memory_type) queryParams.set('memory_type', params.memory_type);
  if (params.identity_id) queryParams.set('identity_id', params.identity_id);
  if (params.min_importance !== undefined) queryParams.set('min_importance', String(params.min_importance));
  if (params.include_superseded) queryParams.set('include_superseded', 'true');
  if (params.limit) queryParams.set('limit', String(params.limit));
  if (params.offset) queryParams.set('offset', String(params.offset));

  const query = queryParams.toString();
  return apiFetch(`/memories/filtered${query ? `?${query}` : ''}`);
}

export async function getMemory(id: number): Promise<MemoryInfo> {
  return apiFetch(`/memories/${id}`);
}

export async function updateMemory(id: number, data: {
  content?: string;
  category?: string;
  tags?: string;
  importance?: number;
  entity_type?: string;
  entity_name?: string;
  valid_from?: string;
  valid_until?: string;
  temporal_type?: string;
}): Promise<MemoryInfo> {
  return apiFetch(`/memories/${id}`, {
    method: 'PUT',
    body: JSON.stringify(data),
  });
}

export async function deleteMemory(id: string): Promise<void> {
  await apiFetch(`/memories/${id}`, { method: 'DELETE' });
}

export async function mergeMemories(memoryIds: number[], mergedContent: string): Promise<{
  success: boolean;
  merged_memory: MemoryInfo;
  superseded_count: number;
}> {
  return apiFetch('/memories/merge', {
    method: 'POST',
    body: JSON.stringify({
      memory_ids: memoryIds,
      merged_content: mergedContent,
      use_max_importance: true,
    }),
  });
}

export async function getMemoryStats(): Promise<MemoryStats> {
  return apiFetch('/memories/stats');
}

export async function exportMemories(identityId?: string): Promise<string> {
  const query = identityId ? `?identity_id=${encodeURIComponent(identityId)}` : '';
  const response = await fetch(`/api/memories/export${query}`, {
    headers: {
      Authorization: `Bearer ${localStorage.getItem('stark_token')}`,
    },
  });
  if (!response.ok) {
    throw new Error('Failed to export memories');
  }
  return response.text();
}

export async function searchMemories(query: string, params: {
  memory_type?: string;
  identity_id?: string;
  min_importance?: number;
  limit?: number;
} = {}): Promise<Array<{ memory: MemoryInfo; rank: number }>> {
  return apiFetch('/memories/search', {
    method: 'POST',
    body: JSON.stringify({
      query,
      ...params,
      limit: params.limit || 20,
    }),
  });
}

// ============================================
// Memory Graph & Association API (Phase 3)
// ============================================

export async function getMemoryGraph(): Promise<MemoryGraphResponse> {
  return apiFetch('/memory/graph');
}

export async function getHybridSearch(query: string, limit = 20): Promise<HybridSearchResponse> {
  return apiFetch(`/memory/hybrid-search?query=${encodeURIComponent(query)}&limit=${limit}`);
}

export async function listAssociations(memoryId: number): Promise<{ success: boolean; associations: MemoryAssociation[] }> {
  return apiFetch(`/memory/associations?memory_id=${memoryId}`);
}

export async function createAssociation(
  sourceMemoryId: number,
  targetMemoryId: number,
  associationType = 'related',
  strength = 0.5,
): Promise<{ success: boolean; id: number }> {
  return apiFetch('/memory/associations', {
    method: 'POST',
    body: JSON.stringify({
      source_memory_id: sourceMemoryId,
      target_memory_id: targetMemoryId,
      association_type: associationType,
      strength,
    }),
  });
}

export async function deleteAssociation(id: number): Promise<{ success: boolean; deleted: boolean }> {
  return apiFetch(`/memory/associations/${id}`, { method: 'DELETE' });
}

export async function getEmbeddingStats(): Promise<EmbeddingStatsResponse> {
  return apiFetch('/memory/embeddings/stats');
}

export async function backfillEmbeddings(): Promise<{ success: boolean; message: string }> {
  return apiFetch('/memory/embeddings/backfill', { method: 'POST' });
}

export async function getCortexBulletin(): Promise<CortexBulletin> {
  return apiFetch('/bulletin');
}
