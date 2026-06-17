# Test Infrastructure: E2E 4-Tier Requirements-Driven Test Suite

This document defines the End-to-End (E2E) testing infrastructure, philosophy, architecture, and coverage thresholds for the Astro-Probe three-tier code visualization and integration system.

---

## Test Philosophy

The Astro-Probe E2E test suite adheres to a **requirement-driven, opaque-box testing philosophy**. The goal is to validate system behavior from the perspective of an end-user, developer, or editor integration, completely decoupled from the internal implementation details of any single module.

The tests specifically validate four core user requirements:
1. **Workspace Management**: Creating, starting, stopping, listing, and deleting workspaces, including verification of lifecycle statuses (`loaded`, `unloaded`, `idle`).
2. **Lineage Graph & Routing**: Correct extraction of Spring MVC web routes, call graphs (incoming/outgoing), and downstream/upstream data-flow lineages (DFG).
3. **Monaco Code Viewer**: Precise coordinate mapping (`filePath`, `startLine`, `startColumn`, `endLine`, `endColumn`) resolved from Fully Qualified Names (FQNs) to ensure syntax highlighting, line centering, and symbol highlighting work seamlessly.
4. **Zed Deep-Linking**: Formatting and validation of `zed://file/...` URI protocols to support jumping from nodes in the DAG visualizer directly into the local editor.

---

## Feature Inventory & Mapping

The 4 core features are mapped to specific test assertions across Tiers 1-3 to ensure comprehensive coverage.

| Feature Area | Tier 1 (Feature Coverage) | Tier 2 (Boundary & Corner Cases) | Tier 3 (Cross-Feature Combinations) |
|---|---|---|---|
| **Workspace Management** | - Creation on valid path<br>- Listing active workspaces<br>- Stopping to reclaim pool<br>- Starting to reload pool<br>- Deletion of database files | - Non-existent directory paths<br>- Empty/missing name parameter<br>- Start/stop on invalid UUIDs<br>- Delete on invalid UUIDs<br>- Re-creating active workspace path | - Stopped Workspace + Graph Query<br>- Deleted Workspace + Symbol Resolution<br>- Idle Workspace Timeout Auto-Resume |
| **Lineage Graph & Routes** | - Outgoing call graph query<br>- Downstream data flow lineage<br>- List Spring MVC routes | - Call graph on non-existent FQN<br>- Lineage on non-existent symbol<br>- Filter routes with no matches | - stopped Workspace + Route Query |
| **Monaco Code Viewer** | - FQN symbol resolution returning precise coordinates | - Invalid FQN formatting<br>- Fallback to top-of-file | - Incremental Modification + Symbol Resolution |
| **Zed Deep-Linking** | - Zed deep link URI generation and format validation | - Empty coordinates handling<br>- Malformed file paths | - Deleted Workspace + Zed Link Request |

---

## Test Architecture

The E2E test suite sits at the top of the Astro-Probe system. Since the React Frontend (Milestone 3) and Middle Layer Server (Milestone 2) are decoupled and implemented in Node.js, the E2E test suite is written in Node.js to match the tech stack of the upper layers.

### Directory Layout

```
visualizers/e2e-tests/
├── package.json           # Node.js dependencies (express, axios)
├── README.md              # Setup and execution instructions
├── test-runner.js         # Process orchestrator and test launcher
├── mock-middle-layer.js   # Mock Middle Layer with Java FQN resolver
└── e2e.test.js            # Tier 1-4 test assertions and cleanup
```

### Test Runner Orchestration & Invocation

The `test-runner.js` script manages the complete test environment lifecycle:
1. **Compilation Check**: It verifies if the Rust backend binary exists under `target/debug/` or `target/release/`. If missing, it invokes `cargo build --bin astro-probe-server`.
2. **Daemon Spawning**: It launches the Rust daemon (`astro-probe-server --port 8080`).
3. **Mock Middle Layer Spawning**: It launches the Node.js Express server (`node mock-middle-layer.js` on port 3000), which proxies requests to the daemon and performs regex-based Java AST parsing.
4. **Health Check Polling**: It polls the health check endpoints (`http://127.0.0.1:8080/health` and `http://127.0.0.1:3000/health`) until they are healthy.
5. **Execution**: It runs `node e2e.test.js`, passing the Middle Layer URL.
6. **Graceful Teardown**: Upon completion, it terminates both background processes via SIGTERM and cleans up any generated database files.

**Command to invoke E2E tests:**
```bash
cd visualizers/e2e-tests
npm install
npm test
```

---

## Real-World Application Scenarios (Tier 4)

Tier 4 focuses on complex developer tasks and system optimizations, validating how components cooperate to solve real-world problems.

### 1. Spring Controller to Repository Flow
- **Scenario**: A developer wants to verify that untrusted HTTP request parameters are correctly traced from a REST controller endpoint down to a database repository save call.
- **Verification**: The E2E client queries `GET /api/workspaces/:id/lineage?node=UserController.createUser#userDto&direction=downstream`. The test asserts that the resulting lineage DAG includes intermediary service nodes (`UserServiceImpl.create#userDto`) and terminates at the database repository call.

### 2. Spring Event Publisher to Listener Flow
- **Scenario**: Java applications often use decoupled event publishers (`ApplicationEventPublisher.publishEvent`) and listeners (`@EventListener`). Standard call graphs fail to track this. Astro-Probe creates virtual call edges to resolve this connection.
- **Verification**: The test suite uses the `complex-spring` project. It queries the call graph for `EventPublisherService.publishEvent(Object)` and asserts that a virtual call edge exists connecting it directly to `NotificationEventListener.onOrderCreated(OrderCreatedEvent)`.

### 3. Executor Async Thread Handover
- **Scenario**: When code transitions execution to another thread using `new Thread(runnable)` or `Executor.execute(runnable)`, data flow and call hierarchy are typically lost. Astro-Probe links these thread boundaries.
- **Verification**: In integration tests, calling `MyThreadCaller.startThread` with `MyRunnable` is analyzed. The database is queried to ensure a call edge exists from `MyThreadCaller.startThread` to `MyRunnable.run` and from `MyThreadCaller.runExecutor` to `MyRunnable.run`.

### 4. Large Project CFA Bypass
- **Scenario**: On large projects with many call sites, context-sensitive 1-CFA points-to analysis can become extremely slow or run out of memory. The solver must fall back to context-insensitive 0-CFA to ensure compilation/analysis completes.
- **Verification**: During workspace creation on a large/medium test target, the solver counts total call sites. If the count exceeds the threshold (e.g. >1000), it switches to 0-CFA. The test asserts that the workspace transitions to `loaded` successfully without timeouts, and that basic reachability queries still work.

### 5. Incremental Code Modification
- **Scenario**: When a developer changes a single file, re-analyzing the entire project is inefficient. The system must hash files, detect the modification, and update the SQLite database incrementally.
- **Verification**: The test suite copies a project path to a temporary folder, registers it, injects a dummy method in `UserController.java`, and re-triggers analysis. It then queries the symbol resolver for the new method and asserts that the resolver returns the correct new file coordinates.

---

## Coverage Thresholds

To maintain high confidence in system stability, the test suite must satisfy the following coverage criteria:

- **Tier 1 (Feature Coverage)**: At least **5 test cases** per feature group.
  - *Workspace Management*: Create Workspace, List Workspaces, Stop Workspace, Start Workspace, Delete Workspace.
  - *Integrations & Code Viewer*: Outgoing Call Graph, Downstream Lineage, Web Routes Query, Monaco Symbol Resolution, Zed Deep Link URI Generation.
- **Tier 2 (Boundary & Corner Cases)**: At least **5 test cases** per feature group.
  - *Workspace Management*: Non-existent Project Path, Missing Workspace Name, Non-existent Workspace ID on Operations, Re-creating Workspace on Active Path, Clean Up Lock Handling.
  - *Integrations & Code Viewer*: Query Non-existent Method, Query Non-existent Variable, Invalid FQN Formatting, Routes Filter Non-matching, CFA Bypass Trigger.
- **Tier 3 (Cross-Feature Combinations)**: Evaluates pairwise state interaction.
  - Stopped Workspace + Graph Query (404 Closed).
  - Deleted Workspace + Symbol Resolution (404 Removed).
  - Idle Workspace + Graph Query (Auto-Resume to Loaded).
  - Incremental Modification + Symbol Resolution (File update and coordinate realignment).
- **Tier 4 (Real-World Application Scenarios)**: At least **5 scenarios** representing production developer journeys.
  - Code Exploration & Call Graph Navigation.
  - Data Flow and Security Audit.
  - Spring Event Lineage Resolution.
  - Editor Collaborative Jump (Zed Integration).
  - Workspace Resource Reclamation.
