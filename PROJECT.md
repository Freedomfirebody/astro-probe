# Project: astro-probe

## Architecture
- **Language-Agnostic Core (`astro-probe-core`)**:
  - `traits.rs`: Core abstractions (`SourceParser`, `DependencyAnalyzer`, `FrameworkAnalyzer`, etc.).
  - `facts.rs`: Relational fact definitions (class, method, allocation, call site, etc.).
  - `cg.rs`: Call graph builder and Andersen's Points-To solver.
  - `dfg.rs`: Data flow graph lineage builder.
  - `query.rs`: DFG query lineage resolution.
- **Database Layer (`astro-probe-db`)**:
  - Managed by SQLite/r2d2 connection pool.
  - Stores parsed facts, call graph edges, data-flow facts, and file metadata (e.g. content hashes).
- **Language Frontends (e.g., `astro-probe-java`)**:
  - Implements `SourceParser` for parsing Java `.java` files.
  - Implements `DependencyAnalyzer` for parsing `.jar` archives.
  - Implements `FrameworkAnalyzer` for Spring DI/MVC framework-specific logic.
- **Workspace Manager / HTTP / MCP Server (`astro-probe-server`)**:
  - Orchestrates frontends using `WorkspaceManager`.
  - Exposes analysis features via HTTP REST APIs and MCP (Model Context Protocol).

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| 1 | Workspace Restructure + Core Abstraction | Multi-crate workspace, core traits, extracted cg/dfg/db/server | None | DONE |
| 2 | Incremental Analysis | Content hash tracking, dirty node propagation, supernode bypass, method summary, transitive reduction | M1 | IN_PROGRESS |
| 3 | Framework Routes & Event Lineage | Spring MVC, Spring Event, @Async, Callback interface | M2 | PLANNED |
| 4 | Build Tools & k-CFA | Maven/Gradle integration, k-CFA (k=1), generic type propagation | M3 | PLANNED |
| 5 | Persistence & Protocol | Workspace persistence, MCP SSE transport, query depth limit & timeout, tracing | M4 | PLANNED |

## Interface Contracts
### `astro-probe-core` ↔ `astro-probe-db`
- `core` writes parsed facts to `db`.
- `core` queries `db` for points-to and DFG resolution.

### `astro-probe-server` ↔ Language Frontends
- Frontend registration: Frontends register with `WorkspaceManager`.
- Parser execution: `WorkspaceManager` calls `SourceParser::parse` on source files.

## Code Layout
- `crates/astro-probe-core/`
- `crates/astro-probe-db/`
- `crates/astro-probe-java/`
- `crates/astro-probe-server/`
- `crates/astro-probe-tests/`
