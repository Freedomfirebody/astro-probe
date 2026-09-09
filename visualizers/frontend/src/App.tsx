import React, { useState, useEffect, useCallback, useRef } from 'react';
import { 
  Play, 
  Square, 
  Trash2, 
  Plus, 
  Folder, 
  Activity, 
  Loader2, 
  AlertCircle,
  CheckCircle2
} from 'lucide-react';
import { GraphVisualizer } from './components/GraphVisualizer';
import { CodeViewer } from './components/CodeViewer';
import { ErrorBoundary } from './components/ErrorBoundary';

interface Workspace {
  id: string | number;
  name: string;
  project_path: string;
  status: 'loaded' | 'unloaded' | 'idle' | string;
}

interface SymbolCoords {
  filePath: string;
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
}

export default function App() {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [selectedWorkspace, setSelectedWorkspace] = useState<Workspace | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Form states
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [newWsName, setNewWsName] = useState('');
  const [newWsPath, setNewWsPath] = useState('');
  const [isCreating, setIsCreating] = useState(false);
  const [creationStep, setCreationStep] = useState(0);

  // Symbol resolution states
  const [searchFqn, setSearchFqn] = useState('com.example.simple.controller.UserController.getUserById(java.lang.Long)');
  const [searchInput, setSearchInput] = useState(searchFqn);
  const [activeFilePath, setActiveFilePath] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState<string>('');
  const [symbolCoords, setSymbolCoords] = useState<SymbolCoords | null>(null);
  const [fileLoading, setFileLoading] = useState(false);
  const resolveRequestRef = useRef<number>(0);

  // Tab and Graph states
  const [activeTab, setActiveTab] = useState<'routes' | 'call-graph' | 'lineage'>('routes');
  const [direction, setDirection] = useState<'incoming' | 'outgoing' | 'upstream' | 'downstream'>('outgoing');
  const [activeFqn, setActiveFqn] = useState<string | null>(null);
  const [graphData, setGraphData] = useState<{ nodes?: string[]; edges?: any[] } | null>(null);

  const selectedWorkspaceId = selectedWorkspace?.id;
  const selectedWorkspaceStatus = selectedWorkspace?.status;

  // Simulation steps for creation progress stepper
  const creationSteps = [
    'Initiating workspace request',
    'Scanning Java source files',
    'Parsing syntax trees & symbol declarations',
    'Generating Call Graph & Data Flow Lineage',
    'Finalizing SQLite database index'
  ];

  // Load workspaces list
  const fetchWorkspaces = async () => {
    setLoading(true);
    try {
      const res = await fetch('/api/workspaces');
      if (!res.ok) throw new Error(`HTTP error ${res.status}`);
      const data = await res.json();
      setWorkspaces(data);
      setError(null);
      
      if (!selectedWorkspace) {
        const urlParams = new URLSearchParams(window.location.search);
        const wsId = urlParams.get('workspaceId');
        if (wsId) {
          const matched = data.find((w: Workspace) => String(w.id) === String(wsId));
          if (matched) {
            setSelectedWorkspace(matched);
          }
        }
      }
      
      if (selectedWorkspace) {
        const updated = data.find((w: Workspace) => String(w.id) === String(selectedWorkspace.id));
        if (updated) {
          if (
            updated.status !== selectedWorkspace.status ||
            updated.name !== selectedWorkspace.name ||
            updated.project_path !== selectedWorkspace.project_path
          ) {
            setSelectedWorkspace(updated);
          }
        }
      }
    } catch (err: any) {
      console.error(err);
      setError('Failed to fetch workspaces from middle-layer server.');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchWorkspaces();
  }, []);

  useEffect(() => {
    const interval = setInterval(() => {
      fetchWorkspaces();
    }, 5000);
    return () => clearInterval(interval);
  }, [selectedWorkspaceId, selectedWorkspace]);

  const handleCreateWorkspace = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newWsName || !newWsPath) return;

    setIsCreating(true);
    setCreationStep(0);

    const stepInterval = setInterval(() => {
      setCreationStep(prev => (prev < creationSteps.length - 1 ? prev + 1 : prev));
    }, 1200);

    try {
      const res = await fetch('/api/workspaces', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: newWsName, project_path: newWsPath })
      });
      clearInterval(stepInterval);

      if (!res.ok) {
        const errData = await res.json();
        throw new Error(errData.error || `Failed to create workspace (status ${res.status})`);
      }

      const newWs = await res.json();
      setCreationStep(creationSteps.length);
      
      setTimeout(() => {
        setIsCreating(false);
        setShowCreateModal(false);
        setNewWsName('');
        setNewWsPath('');
        fetchWorkspaces();
        setSelectedWorkspace(newWs);
      }, 500);

    } catch (err: any) {
      clearInterval(stepInterval);
      setIsCreating(false);
      alert(err.message || 'Error creating workspace.');
    }
  };

  const handleStartWorkspace = async (wsId: string | number) => {
    try {
      const res = await fetch(`/api/workspaces/${wsId}/start`, { method: 'POST' });
      if (!res.ok) throw new Error('Failed to start workspace.');
      await fetchWorkspaces();
    } catch (err: any) {
      alert(err.message);
    }
  };

  const handleStopWorkspace = async (wsId: string | number) => {
    try {
      const res = await fetch(`/api/workspaces/${wsId}/stop`, { method: 'POST' });
      if (!res.ok) throw new Error('Failed to stop workspace.');
      await fetchWorkspaces();
    } catch (err: any) {
      alert(err.message);
    }
  };

  const handleDeleteWorkspace = async (wsId: string | number) => {
    if (!confirm('Are you sure you want to delete this workspace? All database indexes will be wiped.')) return;
    try {
      const res = await fetch(`/api/workspaces/${wsId}`, { method: 'DELETE' });
      if (!res.ok) throw new Error('Failed to delete workspace.');
      if (selectedWorkspace && String(selectedWorkspace.id) === String(wsId)) {
        setSelectedWorkspace(null);
        setActiveFilePath(null);
        setFileContent('');
        setSymbolCoords(null);
      }
      await fetchWorkspaces();
    } catch (err: any) {
      alert(err.message);
    }
  };

  const handleResolveSymbol = useCallback(async (fqn: string) => {
    if (!selectedWorkspace) return;
    setSearchFqn(fqn);
    setFileLoading(true);
    const requestId = ++resolveRequestRef.current;
    try {
      const res = await fetch(`/api/workspaces/${selectedWorkspace.id}/symbol?fqn=${encodeURIComponent(fqn)}`);
      if (requestId !== resolveRequestRef.current) return;
      if (!res.ok) {
        const errData = await res.json();
        throw new Error(errData.error || 'Symbol not found in workspace.');
      }
      const coords: SymbolCoords = await res.json();
      if (requestId !== resolveRequestRef.current) return;
      
      const fileRes = await fetch(`/api/workspaces/${selectedWorkspace.id}/file?filePath=${encodeURIComponent(coords.filePath)}`);
      if (requestId !== resolveRequestRef.current) return;
      if (!fileRes.ok) throw new Error('Failed to retrieve file contents.');
      const fileData = await fileRes.json();
      if (requestId !== resolveRequestRef.current) return;
      
      // Perform state updates atomically after all fetches succeed
      setSymbolCoords(coords);
      setActiveFilePath(coords.filePath);
      setFileContent(fileData.content);
      setActiveFqn(fqn);
    } catch (err: any) {
      if (requestId === resolveRequestRef.current) {
        alert(err.message);
      }
    } finally {
      if (requestId === resolveRequestRef.current) {
        setFileLoading(false);
      }
    }
  }, [selectedWorkspace]);

  useEffect(() => {
    setActiveFqn(null);
    setActiveFilePath(null);
    setFileContent('');
    setSymbolCoords(null);
    setActiveTab('routes');
    setGraphData(null);
  }, [selectedWorkspaceId]);

  useEffect(() => {
    let ignore = false;
    if (!selectedWorkspaceId || selectedWorkspaceStatus !== 'loaded' || activeTab !== 'routes') {
      return;
    }
    const fetchRoutes = async () => {
      try {
        const res = await fetch(`/api/workspaces/${selectedWorkspaceId}/routes`);
        if (!res.ok) throw new Error('Failed to fetch routes');
        const data = await res.json();
        if (ignore) return;
        const routes = data.routes || [];
        const edges = routes.map((route: any) => ({
          source: `${route.http_method} ${route.path}`,
          target: route.controller_method_fqn,
          is_virtual: false,
          type: 'route'
        }));
        const nodes = Array.from(new Set([
          ...routes.map((route: any) => `${route.http_method} ${route.path}`),
          ...routes.map((route: any) => route.controller_method_fqn)
        ]));
        setGraphData({ nodes, edges });
      } catch (err) {
        if (!ignore) {
          console.error('Failed to load routes data:', err);
        }
      }
    };
    fetchRoutes();
    return () => {
      ignore = true;
    };
  }, [selectedWorkspaceId, selectedWorkspaceStatus, activeTab]);

  useEffect(() => {
    let ignore = false;
    if (!selectedWorkspaceId || selectedWorkspaceStatus !== 'loaded' || activeTab === 'routes') {
      return;
    }
    // Clear graphData immediately to avoid displaying old graph during transition
    setGraphData(null);

    const fetchGraph = async () => {
      try {
        if (activeTab === 'call-graph') {
          const targetFqn = searchFqn; // Decoupled from activeFqn to allow fallback rendering
          if (!targetFqn) {
            if (!ignore) setGraphData(null);
            return;
          }
          const dir = direction === 'incoming' || direction === 'upstream' ? 'incoming' : 'outgoing';
          const res = await fetch(`/api/workspaces/${selectedWorkspaceId}/call-graph?method=${encodeURIComponent(targetFqn)}&direction=${dir}`);
          if (ignore) return;
          if (res.ok) {
            const data = await res.json();
            if (ignore) return;
            setGraphData(data);
          } else {
            setGraphData({ edges: [] });
          }
        } else if (activeTab === 'lineage') {
          const targetFqn = searchFqn; // Decoupled from activeFqn to allow fallback rendering
          if (!targetFqn) {
            if (!ignore) setGraphData(null);
            return;
          }
          const dir = direction === 'incoming' || direction === 'upstream' ? 'upstream' : 'downstream';
          const res = await fetch(`/api/workspaces/${selectedWorkspaceId}/lineage?node=${encodeURIComponent(targetFqn)}&direction=${dir}`);
          if (ignore) return;
          if (res.ok) {
            const data = await res.json();
            if (ignore) return;
            setGraphData(data);
          } else {
            setGraphData({ nodes: [], edges: [] });
          }
        }
      } catch (err) {
        if (!ignore) {
          console.error('Failed to load graph data:', err);
        }
      }
    };
    fetchGraph();
    return () => {
      ignore = true;
    };
  }, [selectedWorkspaceId, selectedWorkspaceStatus, activeTab, direction, searchFqn]);

  const handleTabChange = (tab: 'routes' | 'call-graph' | 'lineage') => {
    setActiveTab(tab);
    if (tab === 'call-graph') {
      setDirection(prev => (prev === 'upstream' || prev === 'incoming' ? 'incoming' : 'outgoing'));
    } else if (tab === 'lineage') {
      setDirection(prev => (prev === 'upstream' || prev === 'incoming' ? 'upstream' : 'downstream'));
    }
  };

  const handleNodeClick = useCallback((fqn: string) => {
    if (
      fqn.startsWith('GET ') ||
      fqn.startsWith('POST ') ||
      fqn.startsWith('PUT ') ||
      fqn.startsWith('DELETE ') ||
      fqn.startsWith('PATCH ')
    ) {
      return;
    }
    setSearchInput(fqn);
    handleResolveSymbol(fqn);
  }, [handleResolveSymbol]);

  const renderStatusBadge = (status: string) => {
    switch (status) {
      case 'loaded':
        return (
          <span className="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold bg-emerald-500/10 text-emerald-400 border border-emerald-500/20">
            <span className="w-1.5 h-1.5 mr-1 rounded-full bg-emerald-400 animate-pulse"></span>
            loaded
          </span>
        );
      case 'idle':
        return (
          <span className="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold bg-amber-500/10 text-amber-400 border border-amber-500/20">
            <span className="w-1.5 h-1.5 mr-1 rounded-full bg-amber-400"></span>
            idle
          </span>
        );
      case 'unloaded':
      default:
        return (
          <span className="inline-flex items-center px-2 py-0.5 rounded-full text-xs font-semibold bg-slate-500/10 text-slate-400 border border-slate-500/20">
            <span className="w-1.5 h-1.5 mr-1 rounded-full bg-slate-400"></span>
            unloaded
          </span>
        );
    }
  };

  return (
    <div className="flex flex-col h-screen overflow-hidden bg-slate-950 text-slate-100">
      <header className="flex items-center justify-between px-6 py-4 border-b bg-slate-900/60 border-slate-800 backdrop-blur-md">
        <div className="flex items-center space-x-3">
          <div className="flex items-center justify-center w-8 h-8 rounded-lg bg-blue-600 shadow-lg shadow-blue-500/30">
            <Activity className="w-5 h-5 text-white" />
          </div>
          <div>
            <h1 className="text-lg font-bold tracking-tight bg-gradient-to-r from-blue-400 to-indigo-300 bg-clip-text text-transparent">
              Astro-Probe
            </h1>
            <p className="text-[10px] text-slate-400">Spring Codebase Explorer & Call Graph Analyzer</p>
          </div>
        </div>
        <div className="flex items-center space-x-2 text-xs">
          <span className="px-2 py-1 rounded bg-slate-800 text-slate-300 border border-slate-700">
            API Host: <strong className="text-slate-100">localhost:3000</strong>
          </span>
        </div>
      </header>

      {error && (
        <div className="bg-red-500/10 border-b border-red-500/20 px-6 py-2 text-xs text-red-400 flex items-center space-x-2">
          <AlertCircle className="w-4 h-4 text-red-400 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      <div className="flex flex-1 overflow-hidden">
        <aside className="flex flex-col w-80 border-r border-slate-800 bg-slate-900/40">
          <div className="p-4 border-b border-slate-800/80 flex items-center justify-between">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-slate-400">Workspaces</h2>
            <button 
              onClick={() => setShowCreateModal(true)}
              className="flex items-center space-x-1 px-2.5 py-1.5 rounded-md text-xs font-medium bg-blue-600 hover:bg-blue-500 transition-colors text-white shadow-sm"
            >
              <Plus className="w-3.5 h-3.5" />
              <span>New</span>
            </button>
          </div>

          <div className="flex-1 overflow-y-auto p-3 space-y-2">
            {loading && workspaces.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-8 space-y-2">
                <Loader2 className="w-6 h-6 text-blue-500 animate-spin" />
                <span className="text-xs text-slate-400">Loading workspaces...</span>
              </div>
            ) : workspaces.length === 0 ? (
              <div className="text-center py-8 text-xs text-slate-500">
                No workspaces created yet. Click "New" to scan your first codebase.
              </div>
            ) : (
              workspaces.map((ws) => (
                <div 
                  key={ws.id}
                  onClick={() => setSelectedWorkspace(ws)}
                  className={`group relative flex flex-col p-3 rounded-lg border transition-all cursor-pointer ${
                    selectedWorkspace && String(selectedWorkspace.id) === String(ws.id)
                      ? 'bg-slate-800/80 border-blue-500/50 shadow-md shadow-blue-500/5'
                      : 'bg-slate-900/30 border-slate-800/80 hover:bg-slate-800/40 hover:border-slate-700'
                  }`}
                >
                  <div className="flex items-start justify-between mb-1.5">
                    <span className="text-sm font-semibold truncate pr-4 text-slate-200">
                      {ws.name}
                    </span>
                    {renderStatusBadge(ws.status)}
                  </div>
                  
                  <span className="text-[11px] text-slate-400 truncate mb-3 flex items-center" title={ws.project_path}>
                    <Folder className="w-3 h-3 mr-1 shrink-0 text-slate-500" />
                    {ws.project_path}
                  </span>

                  <div className="flex items-center justify-between border-t border-slate-800/40 pt-2 opacity-80 group-hover:opacity-100 transition-opacity">
                    <div className="flex space-x-1">
                      {ws.status !== 'loaded' ? (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handleStartWorkspace(ws.id);
                          }}
                          className="flex items-center justify-center p-1 rounded hover:bg-slate-800 text-slate-400 hover:text-emerald-400 transition-colors"
                          title="Start / Resume Workspace"
                        >
                          <Play className="w-3.5 h-3.5" />
                        </button>
                      ) : (
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handleStopWorkspace(ws.id);
                          }}
                          className="flex items-center justify-center p-1 rounded hover:bg-slate-800 text-slate-400 hover:text-amber-400 transition-colors"
                          title="Stop / Unload Workspace"
                        >
                          <Square className="w-3.5 h-3.5" />
                        </button>
                      )}
                    </div>
                    
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        handleDeleteWorkspace(ws.id);
                      }}
                      className="flex items-center justify-center p-1 rounded hover:bg-slate-800 text-slate-500 hover:text-red-400 transition-colors"
                      title="Delete Workspace"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>
              ))
            )}
          </div>
        </aside>

        <main className="flex-1 flex flex-col min-w-0 bg-slate-950 relative">
          {!selectedWorkspace ? (
            <div className="flex-1 flex flex-col items-center justify-center p-8 text-center bg-slate-950">
              <div className="max-w-md space-y-4">
                <div className="w-16 h-16 rounded-full bg-slate-900 border border-slate-800 flex items-center justify-center mx-auto text-slate-600">
                  <Activity className="w-8 h-8" />
                </div>
                <h2 className="text-xl font-bold text-slate-200">No Workspace Loaded</h2>
                <p className="text-sm text-slate-400 leading-relaxed">
                  Select a workspace from the sidebar or click "New" to scan and index a Spring Boot codebase.
                </p>
              </div>
            </div>
          ) : (
            <div className="flex-1 flex flex-col overflow-hidden">
              <div className="px-6 py-3 bg-slate-900/20 border-b border-slate-800/80 flex flex-wrap items-center justify-between gap-3">
                <div className="flex items-center space-x-4">
                  <span className="text-xs text-slate-400 flex items-center">
                    Active Workspace: 
                    <strong className="text-slate-200 ml-1.5 font-semibold">{selectedWorkspace.name}</strong>
                  </span>
                </div>

                <div className="flex items-center space-x-2">
                  <button
                    onClick={() => handleTabChange('routes')}
                    className={`px-3 py-1.5 rounded text-xs font-semibold transition-colors ${
                      activeTab === 'routes'
                        ? 'bg-blue-600 text-white'
                        : 'bg-slate-900 border border-slate-800 text-slate-400 hover:text-slate-200'
                    }`}
                  >
                    Routes
                  </button>
                  <button
                    onClick={() => handleTabChange('call-graph')}
                    className={`px-3 py-1.5 rounded text-xs font-semibold transition-colors ${
                      activeTab === 'call-graph'
                        ? 'bg-blue-600 text-white'
                        : 'bg-slate-900 border border-slate-800 text-slate-400 hover:text-slate-200'
                    }`}
                  >
                    Call Graph
                  </button>
                  <button
                    onClick={() => handleTabChange('lineage')}
                    className={`px-3 py-1.5 rounded text-xs font-semibold transition-colors ${
                      activeTab === 'lineage'
                        ? 'bg-blue-600 text-white'
                        : 'bg-slate-900 border border-slate-800 text-slate-400 hover:text-slate-200'
                    }`}
                  >
                    Lineage
                  </button>
                </div>

                {(activeTab === 'call-graph' || activeTab === 'lineage') && (
                  <div className="flex items-center space-x-1">
                    <span className="text-[10px] text-slate-400 uppercase tracking-wider mr-1">Direction:</span>
                    <button
                      onClick={() => setDirection(activeTab === 'call-graph' ? 'outgoing' : 'downstream')}
                      className={`px-2 py-1 rounded-l text-[10px] font-semibold border-y border-l transition-colors ${
                        direction === 'outgoing' || direction === 'downstream'
                          ? 'bg-slate-800 text-slate-100 border-slate-700'
                          : 'bg-slate-900 text-slate-500 border-slate-800'
                      }`}
                    >
                      {activeTab === 'call-graph' ? 'Outgoing' : 'Downstream'}
                    </button>
                    <button
                      onClick={() => setDirection(activeTab === 'call-graph' ? 'incoming' : 'upstream')}
                      className={`px-2 py-1 rounded-r text-[10px] font-semibold border transition-colors ${
                        direction === 'incoming' || direction === 'upstream'
                          ? 'bg-slate-800 text-slate-100 border-slate-700'
                          : 'bg-slate-900 text-slate-500 border-slate-800'
                      }`}
                    >
                      {activeTab === 'call-graph' ? 'Incoming' : 'Upstream'}
                    </button>
                  </div>
                )}

                <div className="flex items-center space-x-2 max-w-xl flex-1 min-w-[280px]">
                  <input
                    type="text"
                    value={searchInput}
                    onChange={(e) => setSearchInput(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') {
                        handleResolveSymbol(searchInput);
                      }
                    }}
                    placeholder="Enter FQN: com.example.Class.method(param)"
                    className="flex-1 px-3 py-1.5 rounded bg-slate-900 border border-slate-800 text-xs text-slate-200 focus:outline-none focus:border-blue-500 placeholder-slate-600"
                  />
                  <button
                    onClick={() => handleResolveSymbol(searchInput)}
                    disabled={fileLoading}
                    className="px-3 py-1.5 rounded bg-blue-600 hover:bg-blue-500 disabled:bg-blue-800 text-xs font-semibold text-white transition-colors"
                  >
                    {fileLoading ? 'Resolving...' : 'Go'}
                  </button>
                </div>
              </div>

              <div className="flex-1 flex flex-col lg:flex-row overflow-y-auto lg:overflow-hidden p-4 gap-4">
                <div className="flex-1 flex flex-col min-h-0">
                  <ErrorBoundary key={`${selectedWorkspaceId}-${activeTab}-${direction}${activeTab !== 'routes' ? `-${searchFqn}` : ''}`}>
                    <GraphVisualizer
                      graphData={graphData}
                      activeFqn={activeFqn}
                      onNodeClick={handleNodeClick}
                    />
                  </ErrorBoundary>
                </div>
                <div className="flex-1 flex flex-col min-h-0">
                  <ErrorBoundary key={`${selectedWorkspaceId}-${activeFilePath}`}>
                    <CodeViewer
                      filePath={activeFilePath}
                      fileContent={fileContent}
                      symbolCoordinate={symbolCoords}
                      projectPath={selectedWorkspace?.project_path || null}
                    />
                  </ErrorBoundary>
                </div>
              </div>
            </div>
          )}
        </main>
      </div>

      {showCreateModal && (
        <div className="fixed inset-0 bg-slate-950/70 backdrop-blur-sm z-50 flex items-center justify-center p-4">
          <div className="w-full max-w-md bg-slate-900 border border-slate-800 rounded-xl shadow-2xl overflow-hidden">
            <div className="px-5 py-4 border-b border-slate-800 bg-slate-900/50 flex items-center justify-between">
              <h3 className="text-sm font-bold text-slate-100">Create New Workspace</h3>
            </div>

            {!isCreating ? (
              <form onSubmit={handleCreateWorkspace} className="p-5 space-y-4">
                <div>
                  <label className="block text-xs font-semibold text-slate-400 mb-1.5">Workspace Name</label>
                  <input
                    type="text"
                    required
                    value={newWsName}
                    onChange={(e) => setNewWsName(e.target.value)}
                    placeholder="e.g. simple-spring-app"
                    className="w-full px-3 py-2 rounded bg-slate-950 border border-slate-800 text-sm text-slate-100 focus:outline-none focus:border-blue-500"
                  />
                </div>
                <div>
                  <label className="block text-xs font-semibold text-slate-400 mb-1.5">Project Path (Absolute)</label>
                  <input
                    type="text"
                    required
                    value={newWsPath}
                    onChange={(e) => setNewWsPath(e.target.value)}
                    placeholder="e.g. C:\path\to\spring-app"
                    className="w-full px-3 py-2 rounded bg-slate-950 border border-slate-800 text-sm text-slate-100 focus:outline-none focus:border-blue-500"
                  />
                </div>
                <button
                  type="submit"
                  className="w-full py-2.5 rounded-lg bg-blue-600 hover:bg-blue-500 font-semibold text-sm text-white transition-colors flex items-center justify-center"
                >
                  Create Workspace
                </button>
              </form>
            ) : (
              <div className="p-6 space-y-6">
                <div className="flex flex-col items-center justify-center space-y-3">
                  <Loader2 className="w-8 h-8 text-blue-500 animate-spin" />
                  <span className="text-xs font-semibold text-slate-300">Synchronous Codebase Analysis in Progress</span>
                </div>
                <div className="space-y-3 border-t border-slate-800 pt-5">
                  {creationSteps.map((step, idx) => {
                    const isPassed = creationStep > idx;
                    const isActive = creationStep === idx;
                    return (
                      <div key={idx} className="flex items-center space-x-3 text-xs">
                        {isPassed ? (
                          <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0" />
                        ) : isActive ? (
                          <Loader2 className="w-4 h-4 text-blue-500 animate-spin shrink-0" />
                        ) : (
                          <div className="w-4 h-4 rounded-full border border-slate-800 flex items-center justify-center text-[9px] text-slate-600 shrink-0">
                            {idx + 1}
                          </div>
                        )}
                        <span className={`${isPassed ? 'text-slate-300' : isActive ? 'text-blue-400 font-semibold' : 'text-slate-600'}`}>
                          {step}
                        </span>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
