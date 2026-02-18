import { useEffect, useRef, useState, useCallback, useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import * as d3 from 'd3';
import { animate } from 'animejs';
import { X, Save, Trash2, Menu, Clock, MessageSquare, Zap, GitBranch } from 'lucide-react';
import Button from '@/components/ui/Button';
import HeartbeatIcon from '@/components/HeartbeatIcon';
import {
  getMindGraph,
  createMindNode,
  updateMindNode,
  deleteMindNode,
  getHeartbeatSessions,
  getHeartbeatConfig,
  updateHeartbeatConfig,
  pulseHeartbeatOnce,
  MindNodeInfo,
  MindConnectionInfo,
  HeartbeatSessionInfo,
} from '@/lib/api';
import { getGateway } from '@/lib/gateway-client';

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

export default function MindMap() {
  const navigate = useNavigate();
  const svgRef = useRef<SVGSVGElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const simulationRef = useRef<d3.Simulation<D3Node, D3Link> | null>(null);
  const longPressRef = useRef<{ timer: number | null; triggered: boolean; startX: number; startY: number }>({
    timer: null, triggered: false, startX: 0, startY: 0
  });
  const draggedRef = useRef(false);

  const [nodes, setNodes] = useState<MindNodeInfo[]>([]);
  const [connections, setConnections] = useState<MindConnectionInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Modal state for editing node body
  const [editingNode, setEditingNode] = useState<MindNodeInfo | null>(null);
  const [editBody, setEditBody] = useState('');

  // Sidebar state
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [heartbeatSessions, setHeartbeatSessions] = useState<HeartbeatSessionInfo[]>([]);
  const [highlightedNodeId, setHighlightedNodeId] = useState<number | null>(null);

  // Hover tooltip state
  const [hoveredNode, setHoveredNode] = useState<MindNodeInfo | null>(null);

  // Heartbeat toggle state
  const [heartbeatEnabled, setHeartbeatEnabled] = useState(false);
  const [heartbeatLoading, setHeartbeatLoading] = useState(false);
  const [nextBeatAt, setNextBeatAt] = useState<string | null>(null);
  const [countdown, setCountdown] = useState<string | null>(null);
  const lastSessionIdRef = useRef<number | null>(null);

  // Derived state for node executions (sessions with mind_node_id)
  const nodeExecutions = useMemo(() => {
    const nodeMap = new Map(nodes.map(n => [n.id, n]));
    return heartbeatSessions
      .filter(s => s.mind_node_id !== null)
      .slice(0, 10)
      .map(session => ({
        ...session,
        node: nodeMap.get(session.mind_node_id!)
      }));
  }, [heartbeatSessions, nodes]);

  // Rainbow swirl animation on heartbeat using anime.js
  const triggerHeartbeatAnimation = useCallback((nodeId: number) => {
    console.log('[Animation] triggerHeartbeatAnimation called for node:', nodeId);

    if (!svgRef.current || !containerRef.current) {
      console.log('[Animation] Missing refs - svgRef:', !!svgRef.current, 'containerRef:', !!containerRef.current);
      return;
    }

    const svg = d3.select(svgRef.current);
    const nodeGroup = svg.select(`g[data-node-id="${nodeId}"]`);
    if (nodeGroup.empty()) {
      console.log('[Animation] Node group not found for nodeId:', nodeId);
      return;
    }
    console.log('[Animation] Found node group, proceeding with animation');

    // Get node position and apply current zoom transform
    const transform = nodeGroup.attr('transform');
    const match = transform?.match(/translate\(([^,]+),([^)]+)\)/);
    if (!match) return;

    const nodeX = parseFloat(match[1]);
    const nodeY = parseFloat(match[2]);

    // Get the current zoom transform from main group
    const mainG = svg.select('g.main-group');
    const mainTransform = mainG.attr('transform');
    let tx = 0, ty = 0, scale = 1;
    if (mainTransform) {
      const translateMatch = mainTransform.match(/translate\(([^,]+),\s*([^)]+)\)/);
      const scaleMatch = mainTransform.match(/scale\(([^)]+)\)/);
      if (translateMatch) {
        tx = parseFloat(translateMatch[1]);
        ty = parseFloat(translateMatch[2]);
      }
      if (scaleMatch) {
        scale = parseFloat(scaleMatch[1]);
      }
    }

    // Calculate screen position of node
    const screenX = tx + nodeX * scale;
    const screenY = ty + nodeY * scale;

    // Create container for animation elements
    const animContainer = document.createElement('div');
    animContainer.style.cssText = `
      position: absolute;
      left: ${screenX}px;
      top: ${screenY}px;
      pointer-events: none;
      z-index: 100;
    `;
    containerRef.current.appendChild(animContainer);

    // Rainbow colors
    const colors = ['#ff0000', '#ff7f00', '#ffff00', '#00ff00', '#0080ff', '#8000ff', '#ff00ff'];

    // Create confetti sprinkles bursting from node
    const numSprinkles = 30;

    for (let i = 0; i < numSprinkles; i++) {
      const sprinkle = document.createElement('div');
      const color = colors[i % colors.length];
      const width = 4 + Math.random() * 3;
      const height = 12 + Math.random() * 8;
      const angle = Math.random() * 360;
      const distance = 80 + Math.random() * 120;
      const endX = Math.cos(angle * Math.PI / 180) * distance;
      const endY = Math.sin(angle * Math.PI / 180) * distance;
      const rotation = Math.random() * 360;

      sprinkle.style.cssText = `
        position: absolute;
        width: ${width}px;
        height: ${height}px;
        background: ${color};
        border-radius: ${width / 2}px;
        left: ${-width / 2}px;
        top: ${-height / 2}px;
        transform: rotate(${rotation}deg);
      `;

      animContainer.appendChild(sprinkle);

      // Animate sprinkle bursting outward
      animate(sprinkle, {
        translateX: [0, endX],
        translateY: [0, endY],
        rotate: [rotation, rotation + (Math.random() - 0.5) * 360],
        opacity: [1, 1, 0],
        scale: [0, 1, 0.5],
        duration: 800 + Math.random() * 400,
        delay: i * 15,
        ease: 'outExpo',
      });
    }

    // Central flash
    const flash = document.createElement('div');
    flash.style.cssText = `
      position: absolute;
      width: 20px;
      height: 20px;
      background: white;
      border-radius: 50%;
      left: -10px;
      top: -10px;
    `;
    animContainer.appendChild(flash);

    animate(flash, {
      scale: [0, 3, 0],
      opacity: [1, 0.8, 0],
      duration: 400,
      ease: 'outExpo',
    });

    // Pulse the actual SVG node
    const nodeCircle = nodeGroup.select('circle');
    const originalRadius = nodeCircle.attr('r');
    nodeCircle
      .transition()
      .duration(150)
      .attr('r', parseFloat(originalRadius) * 1.5)
      .transition()
      .duration(300)
      .attr('r', originalRadius);

    // Cleanup
    setTimeout(() => {
      animContainer.remove();
    }, 1500);
  }, []);

  // Load graph data
  const loadGraph = useCallback(async () => {
    try {
      setLoading(true);
      const graph = await getMindGraph();
      setNodes(graph.nodes);
      setConnections(graph.connections);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to load mind map');
    } finally {
      setLoading(false);
    }
  }, []);

  // Load heartbeat sessions
  const loadHeartbeatSessions = useCallback(async () => {
    try {
      const sessions = await getHeartbeatSessions();
      setHeartbeatSessions(sessions);
      if (sessions.length > 0 && lastSessionIdRef.current === null) {
        lastSessionIdRef.current = sessions[0].id;
      }
    } catch (e) {
      console.error('Failed to load heartbeat sessions:', e);
    }
  }, []);

  // Load heartbeat config
  const loadHeartbeatConfig = useCallback(async () => {
    try {
      const config = await getHeartbeatConfig();
      if (config) {
        setHeartbeatEnabled(config.enabled);
        setNextBeatAt(config.next_beat_at || null);
      }
    } catch (e) {
      console.error('Failed to load heartbeat config:', e);
    }
  }, []);

  // Toggle heartbeat
  const handleHeartbeatToggle = async () => {
    setHeartbeatLoading(true);
    try {
      const newEnabled = !heartbeatEnabled;
      const config = await updateHeartbeatConfig({ enabled: newEnabled });
      setHeartbeatEnabled(newEnabled);
      setNextBeatAt(config.next_beat_at || null);
    } catch (e) {
      console.error('Failed to toggle heartbeat:', e);
    } finally {
      setHeartbeatLoading(false);
    }
  };

  // Pulse once
  const [isPulsing, setIsPulsing] = useState(false);

  const handlePulseOnce = async () => {
    setIsPulsing(true);
    console.log('[MindMap] Pulse once clicked');

    // Ensure gateway is connected before pulsing
    const gateway = getGateway();
    try {
      await gateway.connect();
      console.log('[MindMap] Gateway connected, sending pulse request');
    } catch (e) {
      console.error('[MindMap] Failed to connect gateway before pulse:', e);
    }

    // Fire off the pulse request
    pulseHeartbeatOnce()
      .then(config => {
        console.log('[MindMap] Pulse request sent successfully');
        setNextBeatAt(config.next_beat_at || null);
      })
      .catch(e => console.error('[MindMap] Failed to pulse heartbeat:', e));

    // Disable button for 5 seconds to prevent spam
    setTimeout(() => setIsPulsing(false), 5000);
  };

  // Listen for heartbeat events via WebSocket
  useEffect(() => {
    const gateway = getGateway();
    let mounted = true;

    const handleHeartbeatStarted = (data: unknown) => {
      const event = data as { mind_node_id?: number };
      console.log('[MindMap] Heartbeat started event received:', event);
      if (!mounted) return;
      // Backend sets next_beat_at BEFORE execution, so load config now to update countdown
      loadHeartbeatConfig();
      if (event.mind_node_id) {
        console.log('[MindMap] Triggering animation for node:', event.mind_node_id);
        triggerHeartbeatAnimation(event.mind_node_id);
      }
    };

    const handleHeartbeatCompleted = async (data: unknown) => {
      const event = data as { mind_node_id?: number };
      console.log('[MindMap] Heartbeat completed event received:', event);
      if (!mounted) return;
      // Reload config to get updated next_beat_at
      loadHeartbeatConfig();
      // Refresh sessions list
      try {
        const sessions = await getHeartbeatSessions();
        if (mounted) {
          setHeartbeatSessions(sessions);
          if (sessions.length > 0) {
            lastSessionIdRef.current = sessions[0].id;
          }
        }
      } catch (e) {
        console.error('Failed to refresh sessions:', e);
      }
    };

    // Also listen for pulse_started as fallback (uses first node if no specific node)
    const handlePulseStarted = (data: unknown) => {
      console.log('[MindMap] Heartbeat pulse started:', data);
      if (!mounted) return;
      // Backend sets next_beat_at BEFORE execution, so load config now to update countdown
      loadHeartbeatConfig();
      // Try to trigger animation on trunk node (node id 1) as fallback
      if (nodes.length > 0) {
        const trunkNode = nodes.find(n => n.is_trunk) || nodes[0];
        console.log('[MindMap] Triggering animation on trunk node:', trunkNode.id);
        triggerHeartbeatAnimation(trunkNode.id);
      }
    };

    // Listen for pulse completion (especially errors) and refresh sessions + config
    const handlePulseCompleted = async (data: unknown) => {
      const event = data as { success?: boolean; error?: string };
      if (event.success) {
        console.log('[MindMap] Heartbeat pulse completed successfully');
      } else {
        console.error('[MindMap] Heartbeat pulse FAILED:', event.error);
      }
      // Always refresh sessions list and config after pulse completes
      if (!mounted) return;
      // Reload config to get updated next_beat_at
      loadHeartbeatConfig();
      try {
        const sessions = await getHeartbeatSessions();
        if (mounted) {
          setHeartbeatSessions(sessions);
          if (sessions.length > 0) {
            lastSessionIdRef.current = sessions[0].id;
          }
        }
      } catch (e) {
        console.error('Failed to refresh sessions:', e);
      }
    };

    // Register listeners immediately (gateway will queue if not connected)
    gateway.on('heartbeat_started', handleHeartbeatStarted);
    gateway.on('heartbeat_completed', handleHeartbeatCompleted);
    gateway.on('heartbeat_pulse_started', handlePulseStarted);
    gateway.on('heartbeat_pulse_completed', handlePulseCompleted);
    console.log('[MindMap] Registered heartbeat event listeners');

    // Ensure connection
    gateway.connect().then(() => {
      console.log('[MindMap] Gateway connected, listeners active');
    }).catch(e => {
      console.error('[MindMap] Failed to connect to gateway:', e);
    });

    return () => {
      mounted = false;
      gateway.off('heartbeat_started', handleHeartbeatStarted);
      gateway.off('heartbeat_completed', handleHeartbeatCompleted);
      gateway.off('heartbeat_pulse_started', handlePulseStarted);
      gateway.off('heartbeat_pulse_completed', handlePulseCompleted);
      console.log('[MindMap] Unregistered heartbeat event listeners');
    };
  }, [triggerHeartbeatAnimation, nodes, loadHeartbeatConfig]);

  useEffect(() => {
    loadGraph();
    loadHeartbeatSessions();
    loadHeartbeatConfig();
  }, [loadGraph, loadHeartbeatSessions, loadHeartbeatConfig]);

  // Countdown timer effect
  useEffect(() => {
    if (!nextBeatAt || !heartbeatEnabled) {
      setCountdown(null);
      return;
    }

    let lastFetchTime = 0;
    const FETCH_INTERVAL_MS = 5000; // Poll every 5 seconds when stuck on "soon..."

    const updateCountdown = () => {
      const now = new Date().getTime();
      const target = new Date(nextBeatAt).getTime();
      const diff = target - now;

      if (diff <= 0) {
        setCountdown('soon...');
        // Poll periodically when stuck - backend may not have updated next_beat_at yet
        if (now - lastFetchTime >= FETCH_INTERVAL_MS) {
          lastFetchTime = now;
          loadHeartbeatConfig();
        }
        return;
      }

      const hours = Math.floor(diff / (1000 * 60 * 60));
      const minutes = Math.floor((diff % (1000 * 60 * 60)) / (1000 * 60));
      const seconds = Math.floor((diff % (1000 * 60)) / 1000);

      if (hours > 0) {
        setCountdown(`${hours}h ${minutes}m ${seconds}s`);
      } else if (minutes > 0) {
        setCountdown(`${minutes}m ${seconds}s`);
      } else {
        setCountdown(`${seconds}s`);
      }
    };

    updateCountdown();
    const interval = setInterval(updateCountdown, 1000);

    return () => clearInterval(interval);
  }, [nextBeatAt, heartbeatEnabled, loadHeartbeatConfig]);

  // Handle click on node to open edit modal
  const handleNodeEdit = useCallback((node: D3Node) => {
    const nodeInfo = nodes.find(n => n.id === node.id);
    if (nodeInfo) {
      setEditingNode(nodeInfo);
      setEditBody(nodeInfo.body);
    }
  }, [nodes]);

  // Handle save edit
  const handleSaveEdit = async () => {
    if (!editingNode) return;
    try {
      await updateMindNode(editingNode.id, { body: editBody });
      setEditingNode(null);
      await loadGraph();
    } catch (e) {
      console.error('Failed to update node:', e);
    }
  };

  // Handle add child from edit modal — opens edit on the new child immediately
  const handleAddChild = async () => {
    if (!editingNode) return;
    try {
      const newNode = await createMindNode({ parent_id: editingNode.id });
      await loadGraph();
      // Open edit modal on the new child so user fills in content
      setEditingNode(newNode);
      setEditBody(newNode.body);
    } catch (e) {
      console.error('Failed to create child node:', e);
    }
  };

  // Handle delete node
  const handleDeleteNode = async () => {
    if (!editingNode || editingNode.is_trunk) return;
    try {
      await deleteMindNode(editingNode.id);
      setEditingNode(null);
      await loadGraph();
    } catch (e) {
      console.error('Failed to delete node:', e);
    }
  };

  // Handle drag to update position
  const handleDragEnd = useCallback(async (node: D3Node) => {
    if (node.x !== undefined && node.y !== undefined) {
      try {
        await updateMindNode(node.id, {
          position_x: node.x,
          position_y: node.y,
        });
      } catch (e) {
        console.error('Failed to update position:', e);
      }
    }
  }, []);

  // Format date for display
  const formatDate = (dateStr: string) => {
    const date = new Date(dateStr);
    const now = new Date();
    const diff = now.getTime() - date.getTime();
    const hours = Math.floor(diff / (1000 * 60 * 60));
    const days = Math.floor(hours / 24);

    if (days > 0) {
      return `${days}d ago`;
    } else if (hours > 0) {
      return `${hours}h ago`;
    } else {
      const minutes = Math.floor(diff / (1000 * 60));
      return minutes > 0 ? `${minutes}m ago` : 'just now';
    }
  };

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

    // Prepare data for D3 - pin nodes that have saved positions
    const d3Nodes: D3Node[] = nodes.map(n => ({
      id: n.id,
      body: n.body,
      is_trunk: n.is_trunk,
      x: n.position_x ?? undefined,
      y: n.position_y ?? undefined,
      fx: n.position_x != null ? n.position_x : undefined,
      fy: n.position_y != null ? n.position_y : undefined,
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

    simulationRef.current = simulation;

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
      .attr('cursor', 'pointer')
      .attr('data-node-id', d => d.id);

    // Helper to get node fill color based on trunk status and body content
    const getNodeFill = (d: D3Node, hovered = false) => {
      const hasBody = d.body.trim().length > 0;
      if (d.is_trunk) {
        // Trunk: blue if has body, gray-blue if empty
        return hovered
          ? (hasBody ? '#60a5fa' : '#94a3b8')  // lighter on hover
          : (hasBody ? '#3b82f6' : '#64748b');
      } else {
        // Regular: white if has body, gray if empty
        return hovered
          ? (hasBody ? '#e5e7eb' : '#9ca3af')  // lighter on hover
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

    // Click handler - opens edit modal (with drag guard)
    node.on('click', (event, d) => {
      event.stopPropagation();
      if (draggedRef.current) {
        draggedRef.current = false;
        return;
      }
      if (longPressRef.current.triggered) {
        longPressRef.current.triggered = false;
        return;
      }
      handleNodeEdit(d);
    });

    // Suppress browser context menu on nodes
    node.on('contextmenu', (event: MouseEvent) => {
      event.preventDefault();
    });

    // Drag behavior with drag guard to distinguish click vs drag
    const drag = d3.drag<SVGGElement, D3Node>()
      .on('start', (event, d) => {
        draggedRef.current = false;
        if (!event.active) simulation.alphaTarget(0.3).restart();
        d.fx = d.x;
        d.fy = d.y;
      })
      .on('drag', (event, d) => {
        draggedRef.current = true;
        d.fx = event.x;
        d.fy = event.y;
      })
      .on('end', (event, d) => {
        if (!event.active) simulation.alphaTarget(0);
        // Only save position if actually dragged
        if (draggedRef.current) {
          handleDragEnd(d);
        }
      });

    (node as d3.Selection<SVGGElement, D3Node, SVGGElement, unknown>).call(drag);

    // Disable native long-press context menu on nodes for mobile
    node
      .style('touch-action', 'none')
      .style('-webkit-touch-callout', 'none');

    // Long-press detection for mobile (opens edit modal)
    node.on('touchstart', function(event: TouchEvent, d: D3Node) {
      const touch = event.touches[0];
      longPressRef.current.startX = touch.clientX;
      longPressRef.current.startY = touch.clientY;
      longPressRef.current.triggered = false;

      longPressRef.current.timer = window.setTimeout(() => {
        longPressRef.current.triggered = true;
        // Haptic feedback
        if (navigator.vibrate) navigator.vibrate(50);
        // Open edit modal
        handleNodeEdit(d);
      }, 500);
    });

    node.on('touchmove', function(event: TouchEvent) {
      if (longPressRef.current.timer === null) return;
      const touch = event.touches[0];
      const dx = touch.clientX - longPressRef.current.startX;
      const dy = touch.clientY - longPressRef.current.startY;
      if (Math.sqrt(dx * dx + dy * dy) > 10) {
        window.clearTimeout(longPressRef.current.timer);
        longPressRef.current.timer = null;
      }
    });

    node.on('touchend', function() {
      if (longPressRef.current.timer !== null) {
        window.clearTimeout(longPressRef.current.timer);
        longPressRef.current.timer = null;
      }
    });

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
  }, [loading, nodes, connections, handleNodeEdit, handleDragEnd]);

  // Effect to highlight nodes when hovering over sessions
  useEffect(() => {
    if (!svgRef.current) return;

    const svg = d3.select(svgRef.current);

    // Helper to get stroke color
    const getStrokeColor = (n: MindNodeInfo) => {
      const hasBody = n.body.trim().length > 0;
      if (n.is_trunk) {
        return hasBody ? '#2563eb' : '#475569';
      } else {
        return hasBody ? '#888' : '#4b5563';
      }
    };

    // Reset all nodes to default stroke
    nodes.forEach(n => {
      svg.selectAll(`g[data-node-id="${n.id}"] circle`)
        .attr('stroke-width', 2)
        .attr('stroke', getStrokeColor(n));
    });

    // Highlight the selected node
    if (highlightedNodeId !== null) {
      svg.selectAll(`g[data-node-id="${highlightedNodeId}"] circle`)
        .attr('stroke', '#f59e0b')
        .attr('stroke-width', 4);
    }
  }, [highlightedNodeId, nodes]);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full bg-black">
        <div className="text-gray-400">Loading mind map...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex items-center justify-center h-full bg-black">
        <div className="text-red-400">{error}</div>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-black">
      {/* Header */}
      <div className="p-4 border-b border-gray-800 flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold text-white">Mind Map</h1>
          <p className="text-sm text-gray-400">
            Tap a node to edit, drag to move. Add child nodes from the edit modal.
          </p>
        </div>
        <div className="flex items-center gap-4">
          <div className="flex items-center gap-2">
            {countdown && heartbeatEnabled && (
              <span className="text-sm text-gray-400" title="Time to next pulse">
                {countdown}
              </span>
            )}
            <button
              onClick={() => navigate('/heartbeat')}
              className="group cursor-pointer"
              title="Configure heartbeat"
            >
              <HeartbeatIcon enabled={heartbeatEnabled} size={16} />
            </button>
            <button
              onClick={handleHeartbeatToggle}
              disabled={heartbeatLoading}
              className={`relative w-10 h-5 rounded-full transition-colors ${
                heartbeatEnabled ? 'bg-red-500' : 'bg-gray-600'
              } ${heartbeatLoading ? 'opacity-50 cursor-not-allowed' : 'cursor-pointer'}`}
              title={heartbeatEnabled ? 'Disable heartbeat' : 'Enable heartbeat'}
            >
              <span
                className={`absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full transition-transform ${
                  heartbeatEnabled ? 'translate-x-5' : 'translate-x-0'
                }`}
              />
            </button>
          </div>
          <button
            onClick={() => setSidebarOpen(!sidebarOpen)}
            className="p-2 rounded-lg text-gray-400 hover:text-white hover:bg-gray-800 transition-colors"
            title="Heartbeat History"
          >
            <Menu size={20} />
          </button>
        </div>
      </div>

      <div className="flex-1 flex overflow-hidden">
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

        {/* Sidebar */}
        <div
          className={`w-80 border-l border-gray-800 bg-gray-900 flex flex-col transition-all duration-300 ${
            sidebarOpen ? 'translate-x-0' : 'translate-x-full w-0 border-l-0'
          }`}
          style={{ marginRight: sidebarOpen ? 0 : -320 }}
        >
          {/* Close button header */}
          <div className="p-2 border-b border-gray-800 flex justify-end">
            <button
              onClick={() => setSidebarOpen(false)}
              className="p-1 text-gray-400 hover:text-white"
            >
              <X size={18} />
            </button>
          </div>

          {/* Heartbeat History Section */}
          <div className="flex-1 min-h-0 flex flex-col">
            <div className="px-4 py-2 border-b border-gray-800">
              <h2 className="text-sm font-semibold text-white flex items-center gap-2">
                <Clock size={14} />
                Heartbeat History
              </h2>
            </div>
            <div className="flex-1 overflow-y-auto">
              {heartbeatSessions.length === 0 ? (
                <div className="p-4 text-center text-gray-500 text-sm">
                  No heartbeat sessions yet
                </div>
              ) : (
                <div className="divide-y divide-gray-800">
                  {heartbeatSessions.map((session) => (
                    <div
                      key={session.id}
                      className="p-3 hover:bg-gray-800 cursor-pointer transition-colors"
                      onMouseEnter={() => setHighlightedNodeId(session.mind_node_id)}
                      onMouseLeave={() => setHighlightedNodeId(null)}
                      onClick={() => navigate(`/sessions/${session.id}`)}
                    >
                      <div className="flex items-center justify-between mb-1">
                        <div className="flex items-center gap-2">
                          <Clock size={14} className="text-gray-500" />
                          <span className="text-sm text-gray-300">
                            {formatDate(session.created_at)}
                          </span>
                        </div>
                        {session.mind_node_id && (
                          <span className="text-xs px-2 py-0.5 rounded bg-amber-500/20 text-amber-400">
                            Node #{session.mind_node_id}
                          </span>
                        )}
                      </div>
                      <div className="flex items-center gap-2 text-xs text-gray-500">
                        <MessageSquare size={12} />
                        <span>{session.message_count} messages</span>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* Mind Node Executions Section */}
          <div className="flex-1 min-h-0 flex flex-col border-t border-gray-800">
            <div className="px-4 py-2 border-b border-gray-800">
              <h2 className="text-sm font-semibold text-white flex items-center gap-2">
                <GitBranch size={14} />
                Mind Node Executions
              </h2>
            </div>
            <div className="flex-1 overflow-y-auto">
              {nodeExecutions.length === 0 ? (
                <div className="p-4 text-center text-gray-500 text-sm">
                  No node executions yet
                </div>
              ) : (
                <div className="divide-y divide-gray-800">
                  {nodeExecutions.map((execution) => (
                    <div
                      key={execution.id}
                      className="p-3 hover:bg-gray-800 cursor-pointer transition-colors"
                      onMouseEnter={() => setHighlightedNodeId(execution.mind_node_id)}
                      onMouseLeave={() => setHighlightedNodeId(null)}
                      onClick={() => navigate(`/sessions/${execution.id}`)}
                    >
                      <div className="flex items-center justify-between mb-1">
                        <span className={`text-xs px-2 py-0.5 rounded ${
                          execution.node?.is_trunk
                            ? 'bg-green-500/20 text-green-400'
                            : 'bg-amber-500/20 text-amber-400'
                        }`}>
                          {execution.node?.is_trunk ? 'Trunk' : `Node #${execution.mind_node_id}`}
                        </span>
                        <div className="flex items-center gap-2 text-xs text-gray-500">
                          <span>{formatDate(execution.created_at)}</span>
                          <MessageSquare size={12} />
                          <span>{execution.message_count}</span>
                        </div>
                      </div>
                      {execution.node && (
                        <p className="text-xs text-gray-400 truncate">
                          {execution.node.body || <span className="italic text-gray-600">Empty node</span>}
                        </p>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* Pulse Once Button */}
          <div className="p-4 border-t border-gray-800">
            <Button
              variant="secondary"
              onClick={handlePulseOnce}
              isLoading={isPulsing}
              className="w-full"
            >
              <Zap className="w-4 h-4 mr-2" />
              Pulse Once
            </Button>
          </div>
        </div>
      </div>

      {/* Edit Modal */}
      {editingNode && (
        <div className="fixed inset-0 bg-black/80 flex items-center justify-center z-50">
          <div className="bg-gray-900 rounded-lg p-6 w-full max-w-md mx-4 border border-gray-700">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-lg font-semibold text-white">
                {editingNode.is_trunk ? 'Edit Trunk Node' : 'Edit Node'}
              </h2>
              <button
                onClick={() => setEditingNode(null)}
                className="text-gray-400 hover:text-white"
              >
                <X size={20} />
              </button>
            </div>

            <textarea
              value={editBody}
              onChange={(e) => setEditBody(e.target.value)}
              className="w-full h-32 bg-gray-800 border border-gray-600 rounded-lg p-3 text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-stark-500 resize-none"
              placeholder="Enter node content..."
              autoFocus
            />

            <div className="flex items-center justify-between mt-4">
              <div className="flex gap-2">
                <Button
                  variant="ghost"
                  onClick={handleAddChild}
                  className="text-blue-400 hover:text-blue-300 hover:bg-blue-500/10"
                >
                  <GitBranch size={16} className="mr-2" />
                  Add Child
                </Button>
                {!editingNode.is_trunk && (
                  <Button
                    variant="ghost"
                    onClick={handleDeleteNode}
                    className="text-red-400 hover:text-red-300 hover:bg-red-500/10"
                  >
                    <Trash2 size={16} className="mr-2" />
                    Delete
                  </Button>
                )}
              </div>
              <div className="flex gap-2">
                <Button variant="secondary" onClick={() => setEditingNode(null)}>
                  Cancel
                </Button>
                <Button variant="primary" onClick={handleSaveEdit}>
                  <Save size={16} className="mr-2" />
                  Save
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
