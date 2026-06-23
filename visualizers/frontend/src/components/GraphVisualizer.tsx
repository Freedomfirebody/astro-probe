import { useEffect, useRef, useState } from 'react';
import cytoscape from 'cytoscape';
import dagre from 'cytoscape-dagre';
import { ZoomIn, ZoomOut, Maximize, RefreshCw } from 'lucide-react';

cytoscape.use(dagre);

interface GraphVisualizerProps {
  graphData: {
    nodes?: string[];
    edges?: Array<{
      caller?: string;
      callee?: string;
      from?: string;
      to?: string;
      source?: string;
      target?: string;
      is_virtual?: boolean;
      type?: string;
      edge_type?: string;
    }>;
  } | null;
  activeFqn: string | null;
  onNodeClick: (fqn: string) => void;
}

export function GraphVisualizer({ graphData, activeFqn, onNodeClick }: GraphVisualizerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const cyRef = useRef<cytoscape.Core | null>(null);
  const hoveredNodeRef = useRef<cytoscape.NodeSingular | null>(null);

  // Callback ref pattern to prevent stale closures
  const onNodeClickRef = useRef(onNodeClick);
  useEffect(() => {
    onNodeClickRef.current = onNodeClick;
  }, [onNodeClick]);
  const [tooltip, setTooltip] = useState<{
    x: number;
    y: number;
    visible: boolean;
    fqn: string;
  }>({ x: 0, y: 0, visible: false, fqn: '' });

  const getSimpleName = (fqn: string) => {
    let openParenIndex = fqn.indexOf('(');
    let beforeParams = '';
    let paramsContent = '';
    let afterParams = '';
    
    if (openParenIndex !== -1) {
      beforeParams = fqn.substring(0, openParenIndex);
      const closeParenIndex = fqn.lastIndexOf(')');
      if (closeParenIndex !== -1 && closeParenIndex > openParenIndex) {
        paramsContent = fqn.substring(openParenIndex + 1, closeParenIndex).trim();
        afterParams = fqn.substring(closeParenIndex + 1);
      } else {
        paramsContent = '';
        afterParams = fqn.substring(openParenIndex);
      }
    } else {
      beforeParams = fqn;
      paramsContent = '';
      afterParams = '';
    }

    let hashPart = '';
    const hashInAfterIndex = afterParams.indexOf('#');
    if (hashInAfterIndex !== -1) {
      hashPart = afterParams.substring(hashInAfterIndex);
    } else {
      const hashInBeforeIndex = beforeParams.indexOf('#');
      if (hashInBeforeIndex !== -1) {
        hashPart = beforeParams.substring(hashInBeforeIndex);
        beforeParams = beforeParams.substring(0, hashInBeforeIndex);
      }
    }

    // Parse paramsContent
    let paramsPart = '';
    if (openParenIndex !== -1) {
      if (paramsContent) {
        // Split parameters by comma at the top level
        const splitParams = (paramsStr: string): string[] => {
          const result: string[] = [];
          let current = '';
          let depth = 0;
          for (let i = 0; i < paramsStr.length; i++) {
            const char = paramsStr[i];
            if (char === '<') {
              depth++;
              current += char;
            } else if (char === '>') {
              depth = Math.max(0, depth - 1);
              current += char;
            } else if (char === ',' && depth === 0) {
              result.push(current.trim());
              current = '';
            } else {
              current += char;
            }
          }
          if (current.trim()) {
            result.push(current.trim());
          }
          return result;
        };

        const parsedParams = splitParams(paramsContent);
        const simplifiedParams = parsedParams
          .map(param => param.replace(/[a-zA-Z0-9_$]+\./g, ''))
          .join(', ');
        paramsPart = `(${simplifiedParams})`;
      } else {
        paramsPart = '()';
      }
    }

    // Parse beforeParams
    // Split the class/method path by dot, ignoring dots inside <...> brackets
    const splitBeforeParams = (beforeParamsStr: string): string[] => {
      const result: string[] = [];
      let current = '';
      let depth = 0;
      for (let i = 0; i < beforeParamsStr.length; i++) {
        const char = beforeParamsStr[i];
        if (char === '<') {
          depth++;
          current += char;
        } else if (char === '>') {
          depth = Math.max(0, depth - 1);
          current += char;
        } else if (char === '.' && depth === 0) {
          result.push(current.trim());
          current = '';
        } else {
          current += char;
        }
      }
      if (current.trim()) {
        result.push(current.trim());
      }
      return result;
    };

    const segments = splitBeforeParams(beforeParams);
    const lastSegments = segments.length > 2 ? segments.slice(-2) : segments;
    const processedSegments = lastSegments.map(segment => segment.replace(/[a-zA-Z0-9_$]+\./g, ''));
    const label = processedSegments.join('.');

    return `${label}${paramsPart}${hashPart}`;
  };

  useEffect(() => {
    if (!containerRef.current || !graphData) return;

    const elements: cytoscape.ElementDefinition[] = [];
    const nodeIds = new Set<string>();

    const rawEdges = graphData.edges || [];
    const rawNodes = graphData.nodes || [];

    const inferType = (fqn: string) => {
      if (
        fqn.startsWith('GET ') ||
        fqn.startsWith('POST ') ||
        fqn.startsWith('PUT ') ||
        fqn.startsWith('DELETE ') ||
        fqn.startsWith('PATCH ')
      ) {
        return 'route';
      }
      const lower = fqn.toLowerCase();
      if (lower.includes('controller')) return 'controller';
      if (lower.includes('service')) return 'service';
      if (lower.includes('repository') || lower.includes('repo')) return 'repository';
      return 'default';
    };

    // Add nodes explicitly
    rawNodes.forEach(node => {
      if (!nodeIds.has(node)) {
        nodeIds.add(node);
        elements.push({
          data: { id: node, label: getSimpleName(node), fqn: node, type: inferType(node) }
        });
      }
    });

    // Add nodes/edges from edges
    rawEdges.forEach((edge, index) => {
      const source = edge.caller || edge.from || edge.source;
      const target = edge.callee || edge.to || edge.target;

      if (!source || !target) return;

      if (!nodeIds.has(source)) {
        nodeIds.add(source);
        elements.push({
          data: { id: source, label: getSimpleName(source), fqn: source, type: inferType(source) }
        });
      }
      if (!nodeIds.has(target)) {
        nodeIds.add(target);
        elements.push({
          data: { id: target, label: getSimpleName(target), fqn: target, type: inferType(target) }
        });
      }

      elements.push({
        data: {
          id: `${source}->${target}#${edge.type || edge.edge_type || ''}-${index}`,
          source,
          target,
          isVirtual: edge.is_virtual || false,
          edgeType: edge.type || edge.edge_type || ''
        },
        classes: edge.type || edge.edge_type || ''
      });
    });

    if (cyRef.current) {
      cyRef.current.destroy();
    }

    cyRef.current = cytoscape({
      container: containerRef.current,
      elements,
      boxSelectionEnabled: false,
      autounselectify: true,
      style: [
        {
          selector: 'node',
          style: {
            'content': 'data(label)',
            'text-valign': 'center',
            'text-halign': 'center',
            'background-color': '#0284c7', // Sky-600
            'color': '#ffffff',
            'font-size': '11px',
            'font-family': 'monospace',
            'width': '180px',
            'height': '40px',
            'shape': 'round-rectangle',
            'border-width': 1,
            'border-color': '#0369a1',
            'text-wrap': 'wrap',
            'text-max-width': '170px'
          }
        },
        {
          selector: 'node[type="route"]',
          style: {
            'background-color': '#10b981', // Emerald-500
            'border-color': '#047857'
          }
        },
        {
          selector: 'node[type="controller"]',
          style: {
            'background-color': '#3b82f6', // Blue-500
            'border-color': '#1d4ed8'
          }
        },
        {
          selector: 'node[type="service"]',
          style: {
            'background-color': '#f59e0b', // Amber-500
            'border-color': '#b45309'
          }
        },
        {
          selector: 'node[type="repository"]',
          style: {
            'background-color': '#8b5cf6', // Purple-500
            'border-color': '#6d28d9'
          }
        },
        {
          selector: 'node.active',
          style: {
            'border-color': '#f97316',
            'border-width': 4
          }
        },
        {
          selector: 'edge',
          style: {
            'width': 2,
            'line-color': '#94a3b8', // Slate-400
            'target-arrow-color': '#94a3b8',
            'target-arrow-shape': 'triangle',
            'curve-style': 'bezier',
            'control-point-step-size': 40
          }
        },
        {
          selector: 'edge[isVirtual]',
          style: {
            'line-style': 'dashed',
            'line-color': '#cbd5e1'
          }
        },
        {
          selector: 'edge.PASS_ARG',
          style: {
            'line-color': '#3b82f6',
            'target-arrow-color': '#3b82f6'
          }
        },
        {
          selector: 'edge.PASS_RET',
          style: {
            'line-color': '#10b981',
            'target-arrow-color': '#10b981'
          }
        },
        {
          selector: 'edge.PASS_REC',
          style: {
            'line-color': '#8b5cf6',
            'target-arrow-color': '#8b5cf6'
          }
        },
        {
          selector: 'edge.FIELD_READ',
          style: {
            'line-color': '#eab308',
            'target-arrow-color': '#eab308'
          }
        },
        {
          selector: 'edge.FIELD_WRITE',
          style: {
            'line-color': '#f97316',
            'target-arrow-color': '#f97316'
          }
        },
        {
          selector: 'edge.COPY',
          style: {
            'line-color': '#ec4899',
            'target-arrow-color': '#ec4899'
          }
        }
      ],
      layout: {
        name: 'dagre',
        nodeSep: 60,
        edgeSep: 40,
        rankSep: 100,
        rankDir: 'TB'
      } as any
    });

    // Node click handler
    cyRef.current.on('tap', 'node', (evt) => {
      const node = evt.target;
      onNodeClickRef.current(node.data('fqn'));
    });

    const updateTooltipPosition = () => {
      if (hoveredNodeRef.current) {
        const renderedPos = hoveredNodeRef.current.renderedPosition();
        setTooltip(prev => ({
          ...prev,
          x: renderedPos.x,
          y: renderedPos.y - 25
        }));
      }
    };

    // Tooltip hover handlers
    cyRef.current.on('mouseover', 'node', (evt) => {
      const node = evt.target;
      hoveredNodeRef.current = node;
      const renderedPos = node.renderedPosition();
      setTooltip({
        x: renderedPos.x,
        y: renderedPos.y - 25,
        visible: true,
        fqn: node.data('fqn')
      });
    });

    cyRef.current.on('mouseout', 'node', () => {
      hoveredNodeRef.current = null;
      setTooltip(prev => ({ ...prev, visible: false }));
    });

    // Listen to pan, zoom, and position events
    cyRef.current.on('pan zoom', updateTooltipPosition);
    cyRef.current.on('position', 'node', updateTooltipPosition);

    // Set active node class if specified
    if (activeFqn && cyRef.current) {
      cyRef.current.getElementById(activeFqn).addClass('active');
    }

    return () => {
      if (cyRef.current) {
        cyRef.current.off('pan zoom', updateTooltipPosition);
        cyRef.current.off('position', 'node', updateTooltipPosition);
        cyRef.current.off('tap', 'node');
        cyRef.current.off('mouseover', 'node');
        cyRef.current.off('mouseout', 'node');
        cyRef.current.destroy();
        cyRef.current = null;
      }
      hoveredNodeRef.current = null;
    };
  }, [graphData]);

  // Update active style dynamically if activeFqn changes
  useEffect(() => {
    if (!cyRef.current) return;
    // Remove active class from all nodes
    cyRef.current.nodes().removeClass('active');
    if (activeFqn) {
      cyRef.current.getElementById(activeFqn).addClass('active');
    }
  }, [activeFqn]);

  // Handle ResizeObserver to prevent canvas size collapse
  useEffect(() => {
    if (!containerRef.current || !cyRef.current) return;
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => {
      if (cyRef.current) {
        cyRef.current.resize();
      }
    });
    observer.observe(containerRef.current);
    return () => observer.disconnect();
  }, [graphData]);

  const handleZoomIn = () => {
    const cy = cyRef.current;
    if (cy) {
      const newZoom = cy.zoom() * 1.2;
      cy.zoom({
        level: newZoom,
        renderedPosition: { x: cy.width() / 2, y: cy.height() / 2 }
      });
    }
  };

  const handleZoomOut = () => {
    const cy = cyRef.current;
    if (cy) {
      const newZoom = cy.zoom() * 0.8;
      cy.zoom({
        level: newZoom,
        renderedPosition: { x: cy.width() / 2, y: cy.height() / 2 }
      });
    }
  };

  const handleFit = () => {
    if (cyRef.current) {
      cyRef.current.fit();
    }
  };

  const handleResetLayout = () => {
    if (cyRef.current) {
      cyRef.current.layout({
        name: 'dagre',
        nodeSep: 60,
        edgeSep: 40,
        rankSep: 100,
        rankDir: 'TB'
      } as any).run();
    }
  };

  return (
    <div className="relative w-full h-full border border-slate-700 rounded-lg bg-zinc-950 min-h-0 flex flex-col">
      {/* Toolbar */}
      <div className="absolute top-2 right-2 z-10 flex gap-2 bg-zinc-800/80 backdrop-blur-sm p-1 rounded-md border border-zinc-700">
        <button
          onClick={handleZoomIn}
          className="p-1 text-slate-300 hover:text-white hover:bg-zinc-700 rounded transition"
          title="Zoom In"
        >
          <ZoomIn size={16} />
        </button>
        <button
          onClick={handleZoomOut}
          className="p-1 text-slate-300 hover:text-white hover:bg-zinc-700 rounded transition"
          title="Zoom Out"
        >
          <ZoomOut size={16} />
        </button>
        <button
          onClick={handleFit}
          className="p-1 text-slate-300 hover:text-white hover:bg-zinc-700 rounded transition"
          title="Fit to Canvas"
        >
          <Maximize size={16} />
        </button>
        <button
          onClick={handleResetLayout}
          className="p-1 text-slate-300 hover:text-white hover:bg-zinc-700 rounded transition"
          title="Reset Layout"
        >
          <RefreshCw size={16} />
        </button>
      </div>

      {/* Canvas */}
      <div ref={containerRef} className="flex-1 w-full h-full min-h-0 relative" />

      {/* Tooltip Overlay */}
      {tooltip.visible && (
        <div
          className="absolute z-20 pointer-events-none bg-zinc-900 border border-zinc-700 text-slate-200 px-3 py-1.5 rounded shadow-lg text-xs font-mono max-w-sm break-all"
          style={{
            left: `${tooltip.x}px`,
            top: `${tooltip.y}px`,
            transform: 'translate(-50%, -100%)'
          }}
        >
          {tooltip.fqn}
        </div>
      )}
    </div>
  );
}
