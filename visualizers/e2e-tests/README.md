# Astro-Probe E2E Test Suite & Runner 🚀

This is the requirements-driven E2E test suite for Astro-Probe, testing all three layers of the system:
1. **Bottom Layer (Rust Backend)**: Executed by running `astro-probe-server` as a daemon.
2. **Middle Layer (Business Fusion)**: Mocked using an Express server (`mock-middle-layer.js`) that proxies requests to the Rust backend and implements the Java AST/symbol resolution contract.
3. **Frontend / Editor Integrations (React, Monaco, Zed)**: Simulated by a headless client runner (`e2e.test.js`) asserting API logic, Monaco coordinates, and Zed deep-linking.

---

## 📂 Structure

- `test-runner.js`: Orchestrates the test environment: compiles the Rust daemon if missing, starts the Rust backend, starts the Mock Middle Layer, runs the tests, and cleans up.
- `mock-middle-layer.js`: Implements the Middle Layer contract, with a lightweight Java FQN symbol resolver.
- `e2e.test.js`: Executes all 4 tiers of test cases (Feature Coverage, Boundaries, Combinations, and Real-World Scenarios).

---

## 🛠️ Prerequisites

- **Node.js** (v16+) installed.
- **Cargo** (Rust toolchain) installed.

---

## 🚀 Running the Tests

1. Navigate to this directory:
   ```bash
   cd visualizers/e2e-tests
   ```

2. Install dependencies:
   ```bash
   npm install
   ```

3. Run the orchestrator:
   ```bash
   npm test
   ```

The test runner will compile the daemon if needed, launch all processes, execute all 4 tiers of test cases, and automatically shut down all servers.
