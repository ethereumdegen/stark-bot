import { useEffect, useRef, useState, useCallback } from 'react';
import * as d3 from 'd3';
import { RefreshCw, Database, Search, X, Circle, ArrowRight } from 'lucide-react';
import Button from '@/components/ui/Button';
import { getMemoryGraph, getHybridSearch, getEmbeddingStats, backfillEmbeddings } from '@/lib/api';
import type {
  GraphNode,
  MemoryGraphResponse,
  HybridSearchResponse,
  EmbeddingStatsResponse,
} from '@/types';

// ── D3 wrapper types ──

interface D3Node extends d3.SimulationNodeDatum {
  id: number;
  content: string;
  memory_type: string;
  importance: number;
  fx?: number | null;
  fy?: number | null;
}

interface D3Link extends d3.SimulationLinkDatum<D3Node> {
  source: D3Node | number;
  target: D3Node | number;
  association_type: string;
  strength: number;
}

// ── Color maps ──

const MEMORY_TYPE_COLORS: Record<string, string> = {
  daily_log: '#3b82f6',       // blue-500
  long_term: '#a855f7',       // purple-500
  session_summary: '#22c55e', // green-500
  compaction: '#f59e0b',      // amber-500
};

const MEMORY_TYPE_LABELS: Record<string, string> = {
  daily_log: 'Daily Log',
  long_term: 'Long Term',
  session_summary: 'Session Summary',
  compaction: 'Compaction',
};

const EDGE_TYPE_COLORS: Record<string, string> = {
  related: '#64748b',      // slate-500
  caused_by: '#ef4444',    // red-500
  contradicts: '#f97316',  // orange-500
  supersedes: '#eab308',   // yellow-500
  part_of: '#06b6d4',      // cyan-500
  references: '#8b5cf6',   // violet-500
  temporal: '#14b8a6',     // teal-500
};

const EDGE_TYPE_LABELS: Record<string, string> = {
  related: 'Related',
  caused_by: 'Caused By',
  contradicts: 'Contradicts',
  supersedes: 'Supersedes',
  part_of: 'Part Of',
  references: 'References',
  temporal: 'Temporal',
};

// ── Helpers ──

function nodeColor(memoryType: string): string {
  return MEMORY_TYPE_COLORS[memoryType] ?? '#6b7280';
}

function nodeRadius(importance: number): number {
  const clamped = Math.max(0, Math.min(1, importance));
  return 4 + clamped * 12; // 4px – 16px
}

function edgeColor(associationType: string): string {
  return EDGE_TYPE_COLORS[associationType] ?? '#475569';
}

// ── Component ──

export default function MemoryGraph() {
  const svgRef = useRef<SVGSVGElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const simulationRef = useRef<d3.Simulation<D3Node, D3Link> | null>(null);

  // Data state
  const [graphData, setGraphData] = useState<MemoryGraphResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Sidebar state
  const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<HybridSearchResponse | null>(null);
  const [searchLoading, setSearchLoading] = useState(false);
  const [highlightedNodeIds, setHighlightedNodeIds] = useState<Set<number>>(new Set());
  const highlightedRef = useRef<Set<number>>(new Set());

  // Embedding stats
  const [embeddingStats, setEmbeddingStats] = useState<EmbeddingStatsResponse | null>(null);
  const [backfillLoading, setBackfillLoading] = useState(false);
  const [backfillMessage, setBackfillMessage] = useState<string | null>(null);

  // ── Data loading ──

  const loadGraph = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const data = await getMemoryGraph();
      if (!data.success) {
        setError(data.error ?? 'Failed to load memory graph');
        return;
      }
      setGraphData(data);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load memory graph');
    } finally {
      setLoading(false);
    }
  }, []);

  const loadEmbeddingStats = useCallback(async () => {
    try {
      const stats = await getEmbeddingStats();
      setEmbeddingStats(stats);
    } catch (e) {
      console.error('Failed to load embedding stats:', e);
    }
  }, []);

  const handleRefresh = useCallback(async () => {
    await Promise.all([loadGraph(), loadEmbeddingStats()]);
    setSelectedNode(null);
    setSearchResults(null);
    setHighlightedNodeIds(new Set());
    setSearchQuery('');
  }, [loadGraph, loadEmbeddingStats]);

  const handleBackfill = useCallback(async () => {
    setBackfillLoading(true);
    setBackfillMessage(null);
    try {
      const result = await backfillEmbeddings();
      setBackfillMessage(result.message);
      await loadEmbeddingStats();
    } catch (e) {
      setBackfillMessage(e instanceof Error ? e.message : 'Backfill failed');
    } finally {
      setBackfillLoading(false);
    }
  }, [loadEmbeddingStats]);

  const handleSearch = useCallback(async () => {
    const trimmed = searchQuery.trim();
    if (!trimmed) {
      setSearchResults(null);
      setHighlightedNodeIds(new Set());
      return;
    }
    setSearchLoading(true);
    try {
      const results = await getHybridSearch(trimmed);
      setSearchResults(results);
      const matchIds = new Set<number>(results.results.map((r) => r.memory_id));
      setHighlightedNodeIds(matchIds);
    } catch (e) {
      console.error('Hybrid search failed:', e);
      setSearchResults(null);
      setHighlightedNodeIds(new Set());
    } finally {
      setSearchLoading(false);
    }
  }, [searchQuery]);

  // Initial load
  useEffect(() => {
    loadGraph();
    loadEmbeddingStats();
  }, [loadGraph, loadEmbeddingStats]);

  // Keep ref in sync with state so D3 closures always see the latest value
  useEffect(() => {
    highlightedRef.current = highlightedNodeIds;
  }, [highlightedNodeIds]);

  // ── D3 Force-Directed Graph ──

  useEffect(() => {
    if (loading || !svgRef.current || !containerRef.current || !graphData) return;
    if (graphData.nodes.length === 0) return;

    const svg = d3.select(svgRef.current);
    const container = containerRef.current;
    const width = container.clientWidth;
    const height = container.clientHeight;

    // Clear previous content
    svg.selectAll('*').remove();

    // Main group for zoom / pan
    const g = svg.append('g').attr('class', 'main-group');

    // Zoom behaviour
    const zoom = d3.zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.05, 6])
      .on('zoom', (event) => {
        g.attr('transform', event.transform);
      });
    svg.call(zoom);

    // Centre the view
    svg.call(zoom.transform, d3.zoomIdentity.translate(width / 2, height / 2));

    // Build D3 data
    const d3Nodes: D3Node[] = graphData.nodes.map((n) => ({
      id: n.id,
      content: n.content ?? '',
      memory_type: n.memory_type ?? 'unknown',
      importance: n.importance ?? 0.5,
    }));

    const nodeIdSet = new Set(d3Nodes.map((n) => n.id));

    const d3Links: D3Link[] = graphData.edges
      .filter((e) => nodeIdSet.has(e.source) && nodeIdSet.has(e.target))
      .map((e) => ({
        source: e.source,
        target: e.target,
        association_type: e.association_type,
        strength: e.strength,
      }));

    // Simulation
    const simulation = d3.forceSimulation<D3Node, D3Link>(d3Nodes)
      .force(
        'link',
        d3.forceLink<D3Node, D3Link>(d3Links)
          .id((d) => d.id)
          .distance(80)
          .strength((d) => 0.2 + (d as D3Link).strength * 0.5),
      )
      .force('charge', d3.forceManyBody().strength(-200))
      .force('center', d3.forceCenter(0, 0))
      .force('collide', d3.forceCollide<D3Node>().radius((d) => nodeRadius(d.importance) + 4));

    simulationRef.current = simulation;

    // Arrow-head marker definitions for directed edges
    const defs = g.append('defs');
    Object.entries(EDGE_TYPE_COLORS).forEach(([type, color]) => {
      defs
        .append('marker')
        .attr('id', `arrow-${type}`)
        .attr('viewBox', '0 -5 10 10')
        .attr('refX', 15)
        .attr('refY', 0)
        .attr('markerWidth', 6)
        .attr('markerHeight', 6)
        .attr('orient', 'auto')
        .append('path')
        .attr('d', 'M0,-5L10,0L0,5')
        .attr('fill', color);
    });

    // Draw edges
    const link = g
      .append('g')
      .attr('class', 'links')
      .selectAll('line')
      .data(d3Links)
      .join('line')
      .attr('stroke', (d) => edgeColor(d.association_type))
      .attr('stroke-width', (d) => 1 + d.strength * 2)
      .attr('stroke-opacity', (d) => 0.3 + d.strength * 0.5)
      .attr('marker-end', (d) => `url(#arrow-${d.association_type})`);

    // Draw node groups
    const node = g
      .append('g')
      .attr('class', 'nodes')
      .selectAll<SVGGElement, D3Node>('g')
      .data(d3Nodes)
      .join('g')
      .attr('cursor', 'pointer')
      .attr('data-node-id', (d) => d.id);

    // Node circles
    node
      .append('circle')
      .attr('r', (d) => nodeRadius(d.importance))
      .attr('fill', (d) => nodeColor(d.memory_type))
      .attr('stroke', (d) => {
        const c = d3.color(nodeColor(d.memory_type));
        return c ? c.darker(0.6).toString() : '#333';
      })
      .attr('stroke-width', 1.5);

    // Node labels (truncated content)
    node
      .append('text')
      .text((d) => {
        const maxLen = 14;
        return d.content.length > maxLen ? d.content.slice(0, maxLen) + '...' : d.content;
      })
      .attr('text-anchor', 'middle')
      .attr('dy', (d) => nodeRadius(d.importance) + 14)
      .attr('fill', '#94a3b8')
      .attr('font-size', '10px')
      .style('pointer-events', 'none');

    // Hover effects
    node
      .on('mouseenter', function (_event, d) {
        d3.select(this)
          .select('circle')
          .transition()
          .duration(150)
          .attr('r', nodeRadius(d.importance) + 3)
          .attr('stroke-width', 3)
          .attr('stroke', '#e2e8f0');
      })
      .on('mouseleave', function (_event, d) {
        const isHighlighted = highlightedRef.current.has(d.id);
        d3.select(this)
          .select('circle')
          .transition()
          .duration(150)
          .attr('r', nodeRadius(d.importance))
          .attr('stroke-width', isHighlighted ? 3 : 1.5)
          .attr('stroke', () => {
            if (isHighlighted) return '#fbbf24';
            const c = d3.color(nodeColor(d.memory_type));
            return c ? c.darker(0.6).toString() : '#333';
          });
      });

    // Click to select
    node.on('click', (_event, d) => {
      const original = graphData.nodes.find((n) => n.id === d.id) ?? null;
      setSelectedNode(original);
    });

    // Drag behaviour
    const drag = d3
      .drag<SVGGElement, D3Node>()
      .on('start', (event, d) => {
        if (!event.active) simulation.alphaTarget(0.3).restart();
        d.fx = d.x;
        d.fy = d.y;
      })
      .on('drag', (event, d) => {
        d.fx = event.x;
        d.fy = event.y;
      })
      .on('end', (event, d) => {
        if (!event.active) simulation.alphaTarget(0);
        d.fx = null;
        d.fy = null;
      });

    node.call(drag);

    // Tick
    simulation.on('tick', () => {
      link
        .attr('x1', (d) => (d.source as D3Node).x ?? 0)
        .attr('y1', (d) => (d.source as D3Node).y ?? 0)
        .attr('x2', (d) => (d.target as D3Node).x ?? 0)
        .attr('y2', (d) => (d.target as D3Node).y ?? 0);

      node.attr('transform', (d) => `translate(${d.x ?? 0},${d.y ?? 0})`);
    });

    return () => {
      simulation.stop();
      svg.on('.zoom', null);
    };
    // Note: highlightedNodeIds intentionally excluded to avoid full re-render on search.
    // Highlight updates are handled by the separate effect below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loading, graphData]);

  // ── Highlight matching nodes when search results change ──

  useEffect(() => {
    if (!svgRef.current) return;
    const svg = d3.select(svgRef.current);

    // Reset all nodes to default stroke
    svg.selectAll<SVGGElement, D3Node>('g[data-node-id]').each(function (d) {
      const isHighlighted = highlightedNodeIds.has(d.id);
      d3.select(this)
        .select('circle')
        .transition()
        .duration(200)
        .attr('stroke-width', isHighlighted ? 3 : 1.5)
        .attr('stroke', () => {
          if (isHighlighted) return '#fbbf24'; // amber-400
          const c = d3.color(nodeColor(d.memory_type));
          return c ? c.darker(0.6).toString() : '#333';
        })
        .attr('opacity', () => {
          if (highlightedNodeIds.size === 0) return 1;
          return isHighlighted ? 1 : 0.25;
        });
    });

    // Dim / restore edges
    svg.selectAll<SVGLineElement, D3Link>('.links line').each(function (d) {
      const srcId = typeof d.source === 'object' ? (d.source as D3Node).id : d.source;
      const tgtId = typeof d.target === 'object' ? (d.target as D3Node).id : d.target;
      const show =
        highlightedNodeIds.size === 0 ||
        (highlightedNodeIds.has(srcId as number) && highlightedNodeIds.has(tgtId as number));
      d3.select(this)
        .transition()
        .duration(200)
        .attr('stroke-opacity', show ? 0.3 + d.strength * 0.5 : 0.05);
    });
  }, [highlightedNodeIds]);

  // ── Render ──

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full bg-slate-900">
        <div className="text-slate-400">Loading memory graph...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex items-center justify-center h-full bg-slate-900">
        <div className="text-center">
          <p className="text-red-400 mb-4">{error}</p>
          <Button variant="secondary" onClick={handleRefresh}>
            <RefreshCw size={16} className="mr-2" />
            Retry
          </Button>
        </div>
      </div>
    );
  }

  const nodeCount = graphData?.nodes.length ?? 0;
  const edgeCount = graphData?.edges.length ?? 0;

  return (
    <div className="h-full flex flex-col bg-slate-900">
      {/* ── Top toolbar ── */}
      <div className="px-4 py-3 border-b border-slate-700 flex items-center justify-between flex-shrink-0">
        <div className="flex items-center gap-4">
          <h1 className="text-lg font-semibold text-slate-200">Memory Graph</h1>
          <span className="text-xs text-slate-400">
            {nodeCount} nodes / {edgeCount} edges
          </span>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="secondary" size="sm" onClick={handleRefresh}>
            <RefreshCw size={14} className="mr-1.5" />
            Refresh
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={handleBackfill}
            isLoading={backfillLoading}
          >
            <Database size={14} className="mr-1.5" />
            Backfill Embeddings
          </Button>
        </div>
      </div>

      {/* Backfill result message */}
      {backfillMessage && (
        <div className="px-4 py-2 bg-slate-800 border-b border-slate-700 flex items-center justify-between">
          <span className="text-sm text-slate-300">{backfillMessage}</span>
          <button
            onClick={() => setBackfillMessage(null)}
            className="text-slate-500 hover:text-slate-300"
          >
            <X size={14} />
          </button>
        </div>
      )}

      {/* ── Main content: graph + sidebar ── */}
      <div className="flex-1 flex overflow-hidden">
        {/* Graph canvas */}
        <div ref={containerRef} className="flex-1 relative overflow-hidden">
          <svg
            ref={svgRef}
            className="w-full h-full"
            style={{ background: '#0f172a' }}
          />

          {/* Empty state */}
          {nodeCount === 0 && (
            <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
              <p className="text-slate-500">No memory nodes found. Create memories to populate the graph.</p>
            </div>
          )}
        </div>

        {/* ── Sidebar ── */}
        <div className="w-[300px] flex-shrink-0 border-l border-slate-700 bg-slate-800 flex flex-col overflow-hidden">
          {/* Search */}
          <div className="p-3 border-b border-slate-700">
            <div className="relative">
              <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-slate-500" />
              <input
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') handleSearch();
                }}
                placeholder="Hybrid search memories..."
                className="w-full pl-8 pr-8 py-1.5 text-sm bg-slate-900 border border-slate-600 rounded-md text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-blue-500"
              />
              {searchQuery && (
                <button
                  onClick={() => {
                    setSearchQuery('');
                    setSearchResults(null);
                    setHighlightedNodeIds(new Set());
                  }}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-500 hover:text-slate-300"
                >
                  <X size={14} />
                </button>
              )}
            </div>
            {searchLoading && (
              <p className="text-xs text-slate-500 mt-1">Searching...</p>
            )}
            {searchResults && !searchLoading && (
              <p className="text-xs text-slate-500 mt-1">
                {searchResults.results.length} result{searchResults.results.length !== 1 ? 's' : ''} found
              </p>
            )}
          </div>

          {/* Search results */}
          {searchResults && searchResults.results.length > 0 && (
            <div className="border-b border-slate-700 max-h-48 overflow-y-auto">
              {searchResults.results.map((r) => (
                <button
                  key={r.memory_id}
                  onClick={() => {
                    const matchNode = graphData?.nodes.find((n) => n.id === r.memory_id) ?? null;
                    if (matchNode) setSelectedNode(matchNode);
                  }}
                  className="w-full text-left px-3 py-2 hover:bg-slate-700 transition-colors border-b border-slate-700/50 last:border-b-0"
                >
                  <div className="flex items-center justify-between mb-0.5">
                    <span
                      className="text-xs px-1.5 py-0.5 rounded"
                      style={{
                        backgroundColor: nodeColor(r.memory_type) + '20',
                        color: nodeColor(r.memory_type),
                      }}
                    >
                      {MEMORY_TYPE_LABELS[r.memory_type] ?? r.memory_type}
                    </span>
                    <span className="text-xs text-slate-500">
                      score: {r.rrf_score.toFixed(3)}
                    </span>
                  </div>
                  <p className="text-xs text-slate-300 truncate">{r.content}</p>
                </button>
              ))}
            </div>
          )}

          {/* Selected node details */}
          {selectedNode && (
            <div className="p-3 border-b border-slate-700">
              <div className="flex items-center justify-between mb-2">
                <h3 className="text-sm font-semibold text-slate-200">Node Details</h3>
                <button
                  onClick={() => setSelectedNode(null)}
                  className="text-slate-500 hover:text-slate-300"
                >
                  <X size={14} />
                </button>
              </div>
              <div className="space-y-2">
                <div>
                  <span className="text-xs text-slate-500">ID</span>
                  <p className="text-sm text-slate-300">{selectedNode.id}</p>
                </div>
                <div>
                  <span className="text-xs text-slate-500">Type</span>
                  <div className="mt-0.5">
                    <span
                      className="text-xs px-1.5 py-0.5 rounded"
                      style={{
                        backgroundColor: nodeColor(selectedNode.memory_type) + '20',
                        color: nodeColor(selectedNode.memory_type),
                      }}
                    >
                      {MEMORY_TYPE_LABELS[selectedNode.memory_type] ?? selectedNode.memory_type}
                    </span>
                  </div>
                </div>
                <div>
                  <span className="text-xs text-slate-500">Importance</span>
                  <div className="flex items-center gap-2 mt-0.5">
                    <div className="flex-1 h-1.5 bg-slate-700 rounded-full overflow-hidden">
                      <div
                        className="h-full rounded-full"
                        style={{
                          width: `${Math.round(selectedNode.importance * 100)}%`,
                          backgroundColor: nodeColor(selectedNode.memory_type),
                        }}
                      />
                    </div>
                    <span className="text-xs text-slate-400">
                      {(selectedNode.importance * 100).toFixed(0)}%
                    </span>
                  </div>
                </div>
                <div>
                  <span className="text-xs text-slate-500">Content</span>
                  <p className="text-sm text-slate-300 mt-0.5 whitespace-pre-wrap break-words max-h-40 overflow-y-auto">
                    {selectedNode.content}
                  </p>
                </div>
              </div>
            </div>
          )}

          {/* Embedding stats */}
          {embeddingStats && (
            <div className="p-3 border-b border-slate-700">
              <h3 className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-2">
                Embedding Coverage
              </h3>
              <div className="flex items-center gap-2">
                <div className="flex-1 h-2 bg-slate-700 rounded-full overflow-hidden">
                  <div
                    className="h-full bg-blue-500 rounded-full transition-all"
                    style={{ width: `${embeddingStats.coverage_pct}%` }}
                  />
                </div>
                <span className="text-xs text-slate-400 tabular-nums">
                  {embeddingStats.coverage_pct.toFixed(1)}%
                </span>
              </div>
              <p className="text-xs text-slate-500 mt-1">
                {embeddingStats.with_embeddings} / {embeddingStats.total_memories} memories embedded
              </p>
            </div>
          )}

          {/* Legend: Node types */}
          <div className="p-3 border-b border-slate-700">
            <h3 className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-2">
              Node Types
            </h3>
            <div className="space-y-1.5">
              {Object.entries(MEMORY_TYPE_COLORS).map(([type, color]) => (
                <div key={type} className="flex items-center gap-2">
                  <Circle size={10} fill={color} stroke={color} />
                  <span className="text-xs text-slate-300">
                    {MEMORY_TYPE_LABELS[type] ?? type}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {/* Legend: Edge types */}
          <div className="p-3 flex-1 overflow-y-auto">
            <h3 className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-2">
              Edge Types
            </h3>
            <div className="space-y-1.5">
              {Object.entries(EDGE_TYPE_COLORS).map(([type, color]) => (
                <div key={type} className="flex items-center gap-2">
                  <ArrowRight size={10} color={color} />
                  <span className="text-xs text-slate-300">
                    {EDGE_TYPE_LABELS[type] ?? type}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
