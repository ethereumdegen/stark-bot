import { useEffect, useRef, useState, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import * as d3 from 'd3';
import Button from '@/components/ui/Button';
import {
  getGuestImpulseGraph,
  ImpulseNodeInfo,
  ImpulseConnectionInfo,
} from '@/lib/api';

interface D3Node extends d3.SimulationNodeDatum {
  id: number;
  body: string;
  is_trunk: boolean;
  fx?: number | null;
  fy?: number | null;
}

interface D3Link extends d3.SimulationLinkDatum<D3Node> {
  source: D3Node | number;
  target: D3Node | number;
}

export default function GuestDashboard() {
  const navigate = useNavigate();
  const svgRef = useRef<SVGSVGElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const [nodes, setNodes] = useState<ImpulseNodeInfo[]>([]);
  const [connections, setConnections] = useState<ImpulseConnectionInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Hover tooltip state
  const [hoveredNode, setHoveredNode] = useState<ImpulseNodeInfo | null>(null);

  // Load graph data
  const loadGraph = useCallback(async () => {
    try {
      setLoading(true);
      const graph = await getGuestImpulseGraph();
      setNodes(graph.nodes);
      setConnections(graph.connections);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load impulse map');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadGraph();
  }, [loadGraph]);

  // D3 visualization
  useEffect(() => {
    if (loading || !svgRef.current || !containerRef.current || nodes.length === 0) return;

    const svg = d3.select(svgRef.current);
    const container = containerRef.current;
    const width = container.clientWidth;
    const height = container.clientHeight;

    // Clear previous content
    svg.selectAll('*').remove();

    // Create main group for zoom/pan
    const g = svg.append('g').attr('class', 'main-group');

    // Setup zoom
    const zoom = d3.zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.1, 4])
      .on('zoom', (event) => {
        g.attr('transform', event.transform);
      });

    svg.call(zoom);

    // Center the view initially
    svg.call(zoom.transform, d3.zoomIdentity.translate(width / 2, height / 2));

    // Prepare data for D3
    const d3Nodes: D3Node[] = nodes.map(n => ({
      id: n.id,
      body: n.body,
      is_trunk: n.is_trunk,
      x: n.position_x ?? undefined,
      y: n.position_y ?? undefined,
    }));

    const d3Links: D3Link[] = connections.map(c => ({
      source: c.parent_id,
      target: c.child_id,
    }));

    // Create simulation
    const simulation = d3.forceSimulation<D3Node, D3Link>(d3Nodes)
      .force('link', d3.forceLink<D3Node, D3Link>(d3Links)
        .id(d => d.id)
        .distance(100)
        .strength(0.5))
      .force('charge', d3.forceManyBody().strength(-300))
      .force('center', d3.forceCenter(0, 0))
      .force('collide', d3.forceCollide().radius(40));

    // Draw links
    const link = g.append('g')
      .attr('class', 'links')
      .selectAll('line')
      .data(d3Links)
      .join('line')
      .attr('stroke', '#444')
      .attr('stroke-width', 2)
      .attr('stroke-opacity', 0.6);

    // Draw nodes
    const node = g.append('g')
      .attr('class', 'nodes')
      .selectAll('g')
      .data(d3Nodes)
      .join('g')
      .attr('cursor', 'grab')
      .attr('data-node-id', d => d.id);

    // Helper to get node fill color based on trunk status and body content
    const getNodeFill = (d: D3Node, hovered = false) => {
      const hasBody = d.body.trim().length > 0;
      if (d.is_trunk) {
        return hovered
          ? (hasBody ? '#60a5fa' : '#94a3b8')
          : (hasBody ? '#3b82f6' : '#64748b');
      } else {
        return hovered
          ? (hasBody ? '#e5e7eb' : '#9ca3af')
          : (hasBody ? '#ffffff' : '#6b7280');
      }
    };

    const getNodeStroke = (d: D3Node) => {
      const hasBody = d.body.trim().length > 0;
      if (d.is_trunk) {
        return hasBody ? '#2563eb' : '#475569';
      } else {
        return hasBody ? '#888' : '#4b5563';
      }
    };

    // Node circles
    node.append('circle')
      .attr('r', d => d.is_trunk ? 30 : 20)
      .attr('fill', d => getNodeFill(d))
      .attr('stroke', d => getNodeStroke(d))
      .attr('stroke-width', 2)
      .style('transition', 'r 0.2s ease, fill 0.2s ease');

    // Node labels (body text preview)
    node.append('text')
      .text(d => d.body.slice(0, 10) + (d.body.length > 10 ? '...' : ''))
      .attr('text-anchor', 'middle')
      .attr('dy', d => d.is_trunk ? 50 : 35)
      .attr('fill', '#888')
      .attr('font-size', '12px')
      .style('pointer-events', 'none');

    // Hover effects
    node.on('mouseenter', function(_event, d) {
      d3.select(this).select('circle')
        .transition()
        .duration(200)
        .attr('r', d.is_trunk ? 35 : 25)
        .attr('fill', getNodeFill(d, true));
      // Show tooltip
      const nodeInfo = nodes.find(n => n.id === d.id);
      if (nodeInfo) setHoveredNode(nodeInfo);
    })
    .on('mouseleave', function(_event, d) {
      d3.select(this).select('circle')
        .transition()
        .duration(200)
        .attr('r', d.is_trunk ? 30 : 20)
        .attr('fill', getNodeFill(d, false));
      // Hide tooltip
      setHoveredNode(null);
    });

    // Drag behavior (cosmetic only - no persistence)
    const drag = d3.drag<SVGGElement, D3Node>()
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
        // Keep position fixed after drag (local only, no API call)
        d.fx = d.x;
        d.fy = d.y;
      });

    (node as d3.Selection<SVGGElement, D3Node, SVGGElement, unknown>).call(drag);

    // Update positions on tick
    simulation.on('tick', () => {
      link
        .attr('x1', d => (d.source as D3Node).x ?? 0)
        .attr('y1', d => (d.source as D3Node).y ?? 0)
        .attr('x2', d => (d.target as D3Node).x ?? 0)
        .attr('y2', d => (d.target as D3Node).y ?? 0);

      node.attr('transform', d => `translate(${d.x ?? 0},${d.y ?? 0})`);
    });

    // Cleanup
    return () => {
      simulation.stop();
    };
  }, [loading, nodes, connections]);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-screen bg-black">
        <div className="text-gray-400">Loading impulse map...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center h-screen bg-black gap-4">
        <div className="text-red-400">{error}</div>
        <Button onClick={() => navigate('/')}>
          Back to Login
        </Button>
      </div>
    );
  }

  return (
    <div className="h-screen flex flex-col bg-black">
      {/* Header */}
      <div className="p-4 border-b border-gray-800 flex items-center justify-between">
        <div>
          <div className="flex items-center gap-3">
            <h1 className="text-xl font-semibold text-white">Starkbot's Impulse Map</h1>
            <span className="text-xs px-2 py-1 rounded bg-gray-700 text-gray-300">
              Guest View (Read Only)
            </span>
          </div>
          <p className="text-sm text-gray-400">
            Explore Starkbot's impulse map. Login to make changes.
          </p>
        </div>
        <Button onClick={() => navigate('/')}>
          Login
        </Button>
      </div>

      {/* Canvas */}
      <div ref={containerRef} className="flex-1 relative overflow-hidden">
        <svg
          ref={svgRef}
          className="w-full h-full"
          style={{ background: '#000' }}
        />

        {/* Stats */}
        <div className="absolute bottom-2 right-2 text-xs text-gray-600">
          {nodes.length} nodes, {connections.length} connections
        </div>

        {/* Hover Tooltip */}
        {hoveredNode && (
          <div className="absolute bottom-4 left-1/2 transform -translate-x-1/2 max-w-lg px-4 py-3 bg-gray-900/95 border border-gray-700 rounded-lg shadow-xl pointer-events-none">
            <div className="flex items-center gap-2 mb-1">
              <span className={`text-xs px-2 py-0.5 rounded ${hoveredNode.is_trunk ? 'bg-green-500/20 text-green-400' : 'bg-gray-500/20 text-gray-400'}`}>
                {hoveredNode.is_trunk ? 'Trunk' : `Node #${hoveredNode.id}`}
              </span>
            </div>
            <p className="text-sm text-white whitespace-pre-wrap break-words">
              {hoveredNode.body || <span className="text-gray-500 italic">Empty node</span>}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
