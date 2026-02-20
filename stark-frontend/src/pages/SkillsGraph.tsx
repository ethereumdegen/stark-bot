import { useEffect, useRef, useState, useCallback } from 'react';
import * as d3 from 'd3';
import { RefreshCw, Database, Search, X, Circle, ArrowRight, Link, Menu } from 'lucide-react';
import Button from '@/components/ui/Button';
import {
  getSkillGraph,
  searchSkillsByEmbedding,
  getSkillEmbeddingStats,
  backfillSkillEmbeddings,
  rebuildSkillAssociations,
} from '@/lib/api/skills';
import type {
  SkillGraphNode,
  SkillGraphResponse,
  SkillSearchResponse,
  SkillEmbeddingStatsResponse,
} from '@/types';

// ── D3 wrapper types ──

interface D3Node extends d3.SimulationNodeDatum {
  id: number;
  name: string;
  description: string;
  tags: string[];
  enabled: boolean;
  associationCount: number;
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

const TAG_CATEGORY_COLORS: Record<string, string> = {
  finance: '#f59e0b',    // amber-500
  code: '#3b82f6',       // blue-500
  social: '#a855f7',     // purple-500
  secretary: '#22c55e',  // green-500
};

const TAG_CATEGORY_LABELS: Record<string, string> = {
  finance: 'Finance',
  code: 'Code',
  social: 'Social',
  secretary: 'Secretary',
  other: 'Other',
};

const FINANCE_TAGS = ['crypto', 'defi', 'finance', 'trading', 'swap', 'transfer', 'wallet', 'yield', 'lending', 'bridge', 'payments'];
const CODE_TAGS = ['development', 'git', 'code', 'debugging', 'testing', 'deployment', 'ci-cd', 'devops', 'infrastructure'];
const SOCIAL_TAGS = ['social', 'messaging', 'twitter', 'discord', 'telegram', 'communication', 'social-media'];
const SECRETARY_TAGS = ['secretary', 'productivity', 'notes', 'scheduling', 'cron', 'automation'];

function getTagCategory(tags: string[]): string {
  const lower = tags.map((t) => t.toLowerCase());
  if (lower.some((t) => FINANCE_TAGS.includes(t))) return 'finance';
  if (lower.some((t) => CODE_TAGS.includes(t))) return 'code';
  if (lower.some((t) => SOCIAL_TAGS.includes(t))) return 'social';
  if (lower.some((t) => SECRETARY_TAGS.includes(t))) return 'secretary';
  return 'other';
}

const EDGE_TYPE_COLORS: Record<string, string> = {
  related: '#64748b',      // slate-500
  complement: '#06b6d4',   // cyan-500
  alternative: '#f97316',  // orange-500
  prerequisite: '#ef4444', // red-500
  supersedes: '#eab308',   // yellow-500
  category: '#8b5cf6',     // violet-500
};

const EDGE_TYPE_LABELS: Record<string, string> = {
  related: 'Related',
  complement: 'Complement',
  alternative: 'Alternative',
  prerequisite: 'Prerequisite',
  supersedes: 'Supersedes',
  category: 'Category',
};

// ── Helpers ──

function nodeColor(tags: string[]): string {
  const cat = getTagCategory(tags);
  return TAG_CATEGORY_COLORS[cat] ?? '#6b7280';
}

function nodeRadius(associationCount: number): number {
  return Math.min(20, 8 + associationCount * 3);
}

function edgeColor(associationType: string): string {
  return EDGE_TYPE_COLORS[associationType] ?? '#475569';
}

// ── Component ──

export default function SkillsGraph() {
  const svgRef = useRef<SVGSVGElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const simulationRef = useRef<d3.Simulation<D3Node, D3Link> | null>(null);
  const zoomRef = useRef<d3.ZoomBehavior<SVGSVGElement, unknown> | null>(null);
  const d3NodesRef = useRef<D3Node[]>([]);

  // Data state
  const [graphData, setGraphData] = useState<SkillGraphResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Sidebar state
  const [selectedNode, setSelectedNode] = useState<SkillGraphNode | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<SkillSearchResponse | null>(null);
  const [searchLoading, setSearchLoading] = useState(false);
  const [highlightedNodeIds, setHighlightedNodeIds] = useState<Set<number>>(new Set());
  const highlightedRef = useRef<Set<number>>(new Set());

  // Mobile sidebar
  const [sidebarOpen, setSidebarOpen] = useState(false);

  // Category edge toggle
  const [showCategoryEdges, setShowCategoryEdges] = useState(true);

  // Embedding stats
  const [embeddingStats, setEmbeddingStats] = useState<SkillEmbeddingStatsResponse | null>(null);
  const [backfillLoading, setBackfillLoading] = useState(false);
  const [backfillMessage, setBackfillMessage] = useState<string | null>(null);
  const [rebuildLoading, setRebuildLoading] = useState(false);

  // ── Data loading ──

  const loadGraph = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const data = await getSkillGraph();
      if (!data.success) {
        setError(data.error ?? 'Failed to load skill graph');
        return;
      }
      setGraphData(data);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load skill graph');
    } finally {
      setLoading(false);
    }
  }, []);

  const loadEmbeddingStats = useCallback(async () => {
    try {
      const stats = await getSkillEmbeddingStats();
      setEmbeddingStats(stats);
    } catch (e) {
      console.error('Failed to load skill embedding stats:', e);
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
      const result = await backfillSkillEmbeddings();
      setBackfillMessage(result.message);
      await loadEmbeddingStats();
    } catch (e) {
      setBackfillMessage(e instanceof Error ? e.message : 'Backfill failed');
    } finally {
      setBackfillLoading(false);
    }
  }, [loadEmbeddingStats]);

  const handleRebuildAssociations = useCallback(async () => {
    setRebuildLoading(true);
    setBackfillMessage(null);
    try {
      const result = await rebuildSkillAssociations();
      setBackfillMessage(result.message);
      await loadGraph();
    } catch (e) {
      setBackfillMessage(e instanceof Error ? e.message : 'Association rebuild failed');
    } finally {
      setRebuildLoading(false);
    }
  }, [loadGraph]);

  const handleSearch = useCallback(async () => {
    const trimmed = searchQuery.trim();
    if (!trimmed) {
      setSearchResults(null);
      setHighlightedNodeIds(new Set());
      return;
    }
    setSearchLoading(true);
    try {
      const results = await searchSkillsByEmbedding(trimmed, 10);
      setSearchResults(results);
      const matchIds = new Set<number>(results.results.map((r) => r.skill_id));
      setHighlightedNodeIds(matchIds);
    } catch (e) {
      console.error('Skill search failed:', e);
      setSearchResults(null);
      setHighlightedNodeIds(new Set());
    } finally {
      setSearchLoading(false);
    }
  }, [searchQuery]);

  // Zoom to a specific node by id
  const zoomToNode = useCallback((nodeId: number) => {
    if (!svgRef.current || !zoomRef.current) return;
    const d3Node = d3NodesRef.current.find((n) => n.id === nodeId);
    if (!d3Node || d3Node.x == null || d3Node.y == null) return;

    const svg = d3.select(svgRef.current);
    const container = containerRef.current;
    if (!container) return;
    const width = container.clientWidth;
    const height = container.clientHeight;

    const scale = 1.5;
    const transform = d3.zoomIdentity
      .translate(width / 2 - d3Node.x * scale, height / 2 - d3Node.y * scale)
      .scale(scale);

    svg.transition().duration(500).call(zoomRef.current.transform, transform);
  }, []);

  // Initial load
  useEffect(() => {
    loadGraph();
    loadEmbeddingStats();
  }, [loadGraph, loadEmbeddingStats]);

  // Keep ref in sync with state so D3 closures see latest value
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

    const g = svg.append('g').attr('class', 'main-group');

    // Zoom behaviour
    const zoom = d3.zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.1, 6])
      .on('zoom', (event) => {
        g.attr('transform', event.transform);
      });
    svg.call(zoom);
    zoomRef.current = zoom;

    svg.call(zoom.transform, d3.zoomIdentity.translate(width / 2, height / 2));

    // Count associations per node
    const assocCounts = new Map<number, number>();
    graphData.edges.forEach((e) => {
      assocCounts.set(e.source, (assocCounts.get(e.source) ?? 0) + 1);
      assocCounts.set(e.target, (assocCounts.get(e.target) ?? 0) + 1);
    });

    // Build D3 data
    const d3Nodes: D3Node[] = graphData.nodes.map((n) => ({
      id: n.id,
      name: n.name,
      description: n.description,
      tags: n.tags,
      enabled: n.enabled,
      associationCount: assocCounts.get(n.id) ?? 0,
    }));

    d3NodesRef.current = d3Nodes;
    const nodeIdSet = new Set(d3Nodes.map((n) => n.id));

    const apiLinks: D3Link[] = graphData.edges
      .filter((e) => nodeIdSet.has(e.source) && nodeIdSet.has(e.target))
      .map((e) => ({
        source: e.source,
        target: e.target,
        association_type: e.association_type,
        strength: e.strength,
      }));

    // Synthesize category edges: connect skills sharing the same tag category
    const categoryLinks: D3Link[] = [];
    if (showCategoryEdges) {
      const byCategory = new Map<string, number[]>();
      for (const n of d3Nodes) {
        const cat = getTagCategory(n.tags);
        if (cat === 'other') continue;
        if (!byCategory.has(cat)) byCategory.set(cat, []);
        byCategory.get(cat)!.push(n.id);
      }
      // Build an edge-set of existing API links to avoid duplicates
      const existingPairs = new Set(apiLinks.map(
        (l) => `${typeof l.source === 'object' ? (l.source as D3Node).id : l.source}-${typeof l.target === 'object' ? (l.target as D3Node).id : l.target}`
      ));
      for (const [, ids] of byCategory) {
        // Chain pattern: connect each node to the next (avoids O(n²) clutter)
        for (let i = 0; i < ids.length - 1; i++) {
          const a = ids[i], b = ids[i + 1];
          const key1 = `${a}-${b}`, key2 = `${b}-${a}`;
          if (!existingPairs.has(key1) && !existingPairs.has(key2)) {
            categoryLinks.push({
              source: a, target: b,
              association_type: 'category',
              strength: 0.15,
            });
            existingPairs.add(key1);
          }
        }
      }
    }

    const d3Links: D3Link[] = [...apiLinks, ...categoryLinks];

    // Simulation — spread nodes out more to reduce clumping
    const simulation = d3.forceSimulation<D3Node, D3Link>(d3Nodes)
      .force(
        'link',
        d3.forceLink<D3Node, D3Link>(d3Links)
          .id((d) => d.id)
          .distance(250)
          .strength((d) => 0.08 + (d as D3Link).strength * 0.25),
      )
      .force('charge', d3.forceManyBody().strength(-400).distanceMax(1000))
      .force('center', d3.forceCenter(0, 0).strength(0.02))
      .force('collide', d3.forceCollide<D3Node>().radius((d) => nodeRadius(d.associationCount) + 25).strength(0.8))
      .alphaDecay(0.02);

    simulationRef.current = simulation;

    // Arrow-head markers
    const defs = g.append('defs');
    Object.entries(EDGE_TYPE_COLORS).forEach(([type, color]) => {
      defs
        .append('marker')
        .attr('id', `skill-arrow-${type}`)
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
      .attr('stroke-width', (d) => d.association_type === 'category' ? 1.5 : 1 + d.strength * 2)
      .attr('stroke-opacity', (d) => d.association_type === 'category' ? 0.45 : 0.3 + d.strength * 0.5)
      .attr('stroke-dasharray', (d) => d.association_type === 'category' ? '4 3' : null)
      .attr('marker-end', (d) => d.association_type === 'category' ? null : `url(#skill-arrow-${d.association_type})`);

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
      .attr('r', (d) => nodeRadius(d.associationCount))
      .attr('fill', (d) => d.enabled ? nodeColor(d.tags) : '#4b5563')
      .attr('stroke', (d) => {
        const c = d3.color(d.enabled ? nodeColor(d.tags) : '#4b5563');
        return c ? c.darker(0.6).toString() : '#333';
      })
      .attr('stroke-width', 1.5)
      .attr('opacity', (d) => d.enabled ? 1 : 0.5);

    // Node labels (skill name)
    node
      .append('text')
      .text((d) => d.name)
      .attr('text-anchor', 'middle')
      .attr('dy', (d) => nodeRadius(d.associationCount) + 14)
      .attr('fill', '#94a3b8')
      .attr('font-size', '11px')
      .style('pointer-events', 'none');

    // Hover effects + tooltip
    node
      .on('mouseenter', function (event, d) {
        d3.select(this)
          .select('circle')
          .transition()
          .duration(150)
          .attr('r', nodeRadius(d.associationCount) + 3)
          .attr('stroke-width', 3)
          .attr('stroke', '#e2e8f0');

        const tooltip = tooltipRef.current;
        if (tooltip) {
          const cat = getTagCategory(d.tags);
          const catLabel = TAG_CATEGORY_LABELS[cat] ?? 'Other';
          const desc = d.description.length > 120 ? d.description.slice(0, 120) + '\u2026' : d.description;
          tooltip.innerHTML = `<div style="font-weight:600;color:${nodeColor(d.tags)};margin-bottom:4px">${d.name}</div><div style="color:#94a3b8;font-size:10px;margin-bottom:4px">${catLabel} ${d.enabled ? '' : '(disabled)'}</div><div style="color:#cbd5e1;font-size:11px;line-height:1.4">${desc.replace(/</g, '&lt;')}</div>${d.tags.length > 0 ? `<div style="color:#64748b;font-size:10px;margin-top:4px">${d.tags.join(', ')}</div>` : ''}`;
          tooltip.style.display = 'block';
          const rect = containerRef.current?.getBoundingClientRect();
          if (rect) {
            tooltip.style.left = `${event.clientX - rect.left + 12}px`;
            tooltip.style.top = `${event.clientY - rect.top - 10}px`;
          }
        }
      })
      .on('mousemove', function (event) {
        const tooltip = tooltipRef.current;
        if (tooltip) {
          const rect = containerRef.current?.getBoundingClientRect();
          if (rect) {
            tooltip.style.left = `${event.clientX - rect.left + 12}px`;
            tooltip.style.top = `${event.clientY - rect.top - 10}px`;
          }
        }
      })
      .on('mouseleave', function (_event, d) {
        const isHighlighted = highlightedRef.current.has(d.id);
        d3.select(this)
          .select('circle')
          .transition()
          .duration(150)
          .attr('r', nodeRadius(d.associationCount))
          .attr('stroke-width', isHighlighted ? 3 : 1.5)
          .attr('stroke', () => {
            if (isHighlighted) return '#fbbf24';
            const c = d3.color(d.enabled ? nodeColor(d.tags) : '#4b5563');
            return c ? c.darker(0.6).toString() : '#333';
          });

        const tooltip = tooltipRef.current;
        if (tooltip) tooltip.style.display = 'none';
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loading, graphData, showCategoryEdges]);

  // ── Highlight matching nodes when search results change ──

  useEffect(() => {
    if (!svgRef.current) return;
    const svg = d3.select(svgRef.current);

    svg.selectAll<SVGGElement, D3Node>('g[data-node-id]').each(function (d) {
      const isHighlighted = highlightedNodeIds.has(d.id);
      d3.select(this)
        .select('circle')
        .transition()
        .duration(200)
        .attr('stroke-width', isHighlighted ? 3 : 1.5)
        .attr('stroke', () => {
          if (isHighlighted) return '#fbbf24';
          const c = d3.color(d.enabled ? nodeColor(d.tags) : '#4b5563');
          return c ? c.darker(0.6).toString() : '#333';
        })
        .attr('opacity', () => {
          if (highlightedNodeIds.size === 0) return d.enabled ? 1 : 0.5;
          return isHighlighted ? 1 : 0.15;
        });
    });

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
        <div className="text-slate-400">Loading skill graph...</div>
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
    <div className="h-full flex flex-col bg-slate-900 rounded-lg overflow-hidden border border-slate-700">
      {/* ── Top toolbar ── */}
      <div className="px-3 md:px-4 py-3 border-b border-slate-700 flex items-center justify-between flex-shrink-0 gap-2">
        <div className="flex items-center gap-2 md:gap-4 min-w-0">
          <h2 className="text-base md:text-lg font-semibold text-slate-200 whitespace-nowrap">Skills Graph</h2>
          <span className="text-xs text-slate-400 whitespace-nowrap">
            {nodeCount} nodes / {edgeCount} edges
          </span>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="secondary" size="sm" onClick={handleRefresh}>
            <RefreshCw size={14} className="md:mr-1.5" />
            <span className="hidden md:inline">Refresh</span>
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={handleBackfill}
            isLoading={backfillLoading}
            className="hidden md:flex"
          >
            <Database size={14} className="mr-1.5" />
            Backfill
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={handleRebuildAssociations}
            isLoading={rebuildLoading}
            className="hidden md:flex"
          >
            <Link size={14} className="mr-1.5" />
            Rebuild
          </Button>
          <button
            onClick={() => setSidebarOpen(!sidebarOpen)}
            className="md:hidden p-2 text-slate-400 hover:text-white hover:bg-slate-700 rounded-lg transition-colors"
          >
            <Menu size={18} />
          </button>
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

          {/* Hover tooltip */}
          <div
            ref={tooltipRef}
            style={{
              display: 'none',
              position: 'absolute',
              pointerEvents: 'none',
              maxWidth: 280,
              padding: '8px 10px',
              backgroundColor: '#1e293b',
              border: '1px solid #334155',
              borderRadius: 6,
              boxShadow: '0 4px 12px rgba(0,0,0,0.4)',
              zIndex: 50,
              fontSize: 12,
            }}
          />

          {/* Empty state */}
          {nodeCount === 0 && (
            <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
              <p className="text-slate-500">No skills found. Upload skills to populate the graph.</p>
            </div>
          )}
        </div>

        {/* ── Sidebar backdrop (mobile) ── */}
        {sidebarOpen && (
          <div
            className="md:hidden fixed inset-0 bg-black/50 z-30"
            onClick={() => setSidebarOpen(false)}
          />
        )}

        {/* ── Sidebar ── */}
        <div className={`
          fixed md:relative right-0 top-0 h-full z-40
          w-[280px] md:w-[280px] flex-shrink-0 border-l border-slate-700 bg-slate-800 flex flex-col overflow-hidden
          transition-transform duration-200 ease-in-out
          ${sidebarOpen ? 'translate-x-0' : 'translate-x-full'} md:translate-x-0
        `}>
          {/* Mobile sidebar header */}
          <div className="md:hidden flex items-center justify-between px-3 py-2 border-b border-slate-700">
            <span className="text-sm font-semibold text-slate-300">Controls</span>
            <button
              onClick={() => setSidebarOpen(false)}
              className="p-1 text-slate-400 hover:text-white"
            >
              <X size={16} />
            </button>
          </div>

          {/* Mobile-only action buttons */}
          <div className="md:hidden flex gap-2 px-3 py-2 border-b border-slate-700">
            <Button
              variant="secondary"
              size="sm"
              onClick={handleBackfill}
              isLoading={backfillLoading}
              className="flex-1 text-xs"
            >
              <Database size={12} className="mr-1" />
              Backfill
            </Button>
            <Button
              variant="secondary"
              size="sm"
              onClick={handleRebuildAssociations}
              isLoading={rebuildLoading}
              className="flex-1 text-xs"
            >
              <Link size={12} className="mr-1" />
              Rebuild
            </Button>
          </div>

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
                placeholder="Search skills..."
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
              {searchResults.results.map((r, idx) => (
                <button
                  key={r.skill_id}
                  onClick={() => {
                    const matchNode = graphData?.nodes.find((n) => n.id === r.skill_id) ?? null;
                    if (matchNode) setSelectedNode(matchNode);
                    zoomToNode(r.skill_id);
                  }}
                  className="w-full text-left px-3 py-2 hover:bg-slate-700 transition-colors border-b border-slate-700/50 last:border-b-0"
                >
                  <div className="flex items-center justify-between mb-0.5">
                    <span className="text-xs font-medium text-slate-200">{r.name}</span>
                    <span className="text-xs text-slate-500">#{idx + 1}</span>
                  </div>
                  <p className="text-xs text-slate-400 line-clamp-2">{r.description}</p>
                  <div className="mt-1 h-1 bg-slate-700 rounded-full overflow-hidden">
                    <div
                      className="h-full bg-amber-500/60 rounded-full"
                      style={{ width: `${Math.round(r.similarity * 100)}%` }}
                    />
                  </div>
                  <span className="text-xs text-slate-500">{Math.round(r.similarity * 100)}% match</span>
                </button>
              ))}
            </div>
          )}

          {/* Selected node details */}
          {selectedNode && (
            <div className="p-3 border-b border-slate-700">
              <div className="flex items-center justify-between mb-2">
                <h3 className="text-sm font-semibold text-slate-200">Skill Details</h3>
                <button
                  onClick={() => setSelectedNode(null)}
                  className="text-slate-500 hover:text-slate-300"
                >
                  <X size={14} />
                </button>
              </div>
              <div className="space-y-2">
                <div className="flex items-center gap-2">
                  <span
                    className="w-3 h-3 rounded-full"
                    style={{ backgroundColor: nodeColor(selectedNode.tags) }}
                  />
                  <span className="text-sm font-medium text-white">{selectedNode.name}</span>
                  <span className={`text-xs px-1.5 py-0.5 rounded ${selectedNode.enabled ? 'bg-green-500/20 text-green-400' : 'bg-slate-700 text-slate-400'}`}>
                    {selectedNode.enabled ? 'Enabled' : 'Disabled'}
                  </span>
                </div>
                <p className="text-sm text-slate-300 whitespace-pre-wrap break-words max-h-32 overflow-y-auto">
                  {selectedNode.description}
                </p>
                {selectedNode.tags.length > 0 && (
                  <div className="flex flex-wrap gap-1">
                    {selectedNode.tags.map((tag) => (
                      <span key={tag} className="text-xs px-1.5 py-0.5 bg-stark-500/10 text-stark-400 rounded">
                        {tag}
                      </span>
                    ))}
                  </div>
                )}
                <button
                  onClick={() => zoomToNode(selectedNode.id)}
                  className="text-xs text-blue-400 hover:text-blue-300 transition-colors"
                >
                  Center on graph
                </button>
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
                    className="h-full bg-amber-500 rounded-full transition-all"
                    style={{ width: `${embeddingStats.coverage_percent}%` }}
                  />
                </div>
                <span className="text-xs text-slate-400 tabular-nums">
                  {embeddingStats.coverage_percent.toFixed(1)}%
                </span>
              </div>
              <p className="text-xs text-slate-500 mt-1">
                {embeddingStats.skills_with_embeddings} / {embeddingStats.total_skills} skills embedded
              </p>
            </div>
          )}

          {/* Legend: Node categories */}
          <div className="p-3 border-b border-slate-700">
            <h3 className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-2">
              Skill Categories
            </h3>
            <div className="space-y-1.5">
              {Object.entries(TAG_CATEGORY_COLORS).map(([cat, color]) => (
                <div key={cat} className="flex items-center gap-2">
                  <Circle size={10} fill={color} stroke={color} />
                  <span className="text-xs text-slate-300">
                    {TAG_CATEGORY_LABELS[cat] ?? cat}
                  </span>
                </div>
              ))}
              <div className="flex items-center gap-2">
                <Circle size={10} fill="#6b7280" stroke="#6b7280" />
                <span className="text-xs text-slate-300">Other</span>
              </div>
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
                  {type === 'category' ? (
                    <label className="flex items-center gap-2 cursor-pointer">
                      <svg width="16" height="10" className="shrink-0">
                        <line
                          x1="0" y1="5" x2="16" y2="5"
                          stroke={showCategoryEdges ? color : '#475569'}
                          strokeWidth="1.5"
                          strokeDasharray="4 3"
                          strokeOpacity={showCategoryEdges ? 0.8 : 0.3}
                        />
                      </svg>
                      <input
                        type="checkbox"
                        checked={showCategoryEdges}
                        onChange={(e) => setShowCategoryEdges(e.target.checked)}
                        className="w-3 h-3 rounded border-slate-600 bg-slate-800 accent-violet-500"
                      />
                      <span className="text-xs text-slate-300">
                        {EDGE_TYPE_LABELS[type]}
                      </span>
                    </label>
                  ) : (
                    <>
                      <ArrowRight size={10} color={color} />
                      <span className="text-xs text-slate-300">
                        {EDGE_TYPE_LABELS[type] ?? type}
                      </span>
                    </>
                  )}
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
