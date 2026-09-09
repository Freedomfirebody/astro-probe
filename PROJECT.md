# Project: Astro-Probe Three-Tier Visualization and Integration System

## Architecture
The system consists of three decoupled layers plus editor integration:
1. **Bottom Layer (Existing Astro-Probe Backend)**
   - Rust daemon providing REST APIs: `/api/workspaces`, `/api/workspaces/:id/call-graph`, `/api/workspaces/:id/lineage`, `/api/workspaces/:id/routes`.
   - Persists static analysis facts, call graph edges, and DFG lineage edges in workspace-specific SQLite databases.
2. **Middle Layer (Business Fusion / Node.js Server)**
   - Created in `visualizers/server`.
   - Node.js server that acts as a business hub.
   - Proxies workspace commands (create, start, stop, delete, list) and graph queries to the Rust backend.
   - AST-based Java source parser and symbol resolver: reads local source files, extracts exact start/end line and column info for classes, methods, and variables based on their FQNs.
   - Serves frontend static assets.
3. **Frontend Layer (UI / Rendering)**
   - Created in `visualizers/frontend`.
   - Single-page web application (React, Tailwind CSS, Monaco Editor, Cytoscape.js/D3.js).
   - Dashboard: workspace management control center.
   - Interactive DAG visualizer: zoom, pan, hover info for call graphs and lineages.
   - Code viewer: Monaco Editor with syntax highlighting, line centering, and symbol highlighting.
4. **Zed Extension Integration**
   - Created in `visualizers/zed-extension`.
   - Triggers manual re-analysis commands.
   - Registers current workspaces and opens the web visualizer URL.
   - Supports deep-linking (`zed://file/...`) for node-to-code jumping.

## Code Layout
- `visualizers/` - Root directory for visualization components.
- `visualizers/server/` - Node.js Express server.
- `visualizers/frontend/` - Vite/React Single-Page App.
- `visualizers/zed-extension/` - Zed lightweight plugin.

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| 1 | E2E Testing Infrastructure | Setup E2E requirements-driven test suite (Tiers 1-4). Outputs: E2E Test Suite and Mock Infrastructure under `visualizers/e2e-tests`. | None | DONE |
| 2 | Middle Layer Server & Resolver | Node.js Express server with symbol resolver API | M1 | DONE |
| 3 | Frontend Layer Web Dashboard | React UI with Cytoscape/D3 DAG and Monaco code viewer | M2 | DONE |
| 4 | Zed Extension Integration | Zed plugin with deep linking (`zed://file/...`) | M3 | DONE |
| 5 | E2E Integration and Pass | Run E2E verification and Forensic Audit | M4 | DONE |
| 6 | Root README.md updates | Document visualizers folder structure, three-tier architecture, E2E tests, and dev servers. | M5 | DONE |
| 7 | Create docs/zed-plugin.md | Create bilingual user manual for Zed plugin setup, registration, and deep-linking. | M6 | DONE |


## Interface Contracts
### Middle Layer ↔ Bottom Layer (Rust Backend API)
- Proxy all requests under `/api/workspaces` to the Rust daemon.
- Read metadata and workspace paths directly.

### Middle Layer ↔ Frontend
- `GET /api/workspaces` - Retrieve workspaces.
- `POST /api/workspaces` - Create workspace.
- `DELETE /api/workspaces/:id` - Delete workspace.
- `POST /api/workspaces/:id/start` - Start workspace analysis.
- `POST /api/workspaces/:id/stop` - Stop workspace analysis.
- `GET /api/workspaces/:id/call-graph?method=...&direction=...` - Query call graph.
- `GET /api/workspaces/:id/lineage?node=...&direction=...` - Query lineage DAG.
- `GET /api/workspaces/:id/routes?path=...&http_method=...` - Query web routes.
- `GET /api/workspaces/:id/symbol?fqn=...` - Resolve symbol location:
  - Request: symbol FQN (e.g. `com.test.Client.main()#input`).
  - Response: `{ filePath: string, startLine: number, startColumn: number, endLine: number, endColumn: number }`.
