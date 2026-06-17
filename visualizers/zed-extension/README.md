# Astro-Probe Zed Extension

Zed integration for the **Astro-Probe** code lineage and call-graph visualization system.

This extension bridges the Zed editor with the Astro-Probe middle-layer HTTP daemon, enabling seamless project workspace registration, dependency and data-flow call-graph visualization, and interactive code-to-visualizer navigation.

## Extension Architecture

The Astro-Probe extension uses a hybrid architecture:
1. **Wasm Bootstrapper (`extension.wasm`)**: A lightweight Rust-based WebAssembly module compiled using the `zed_extension_api`. Since the Wasm sandboxed environment restricts direct network sockets and native program execution, the bootstrapper locates Node.js on the host path and spawns the bundled JS LSP adapter.
2. **LSP Adapter (`lsp-adapter.js`)**: Runs natively on the host under Node.js. It implements the Language Server Protocol (LSP) and translates editor command requests (via `workspace/executeCommand`) into REST HTTP requests directed to the Astro-Probe Middle Layer daemon.

---

## File Structure

```
visualizers/zed-extension/
├── extension.toml         # Zed extension metadata and adapter config
├── Cargo.toml             # Rust WASM compiler setup
├── src/
│   └── lib.rs             # WASM bootstrapper entrypoint
├── package.json           # Node dependency list for the LSP adapter
├── lsp-adapter.js         # Language Server Protocol adapter logic
├── keymaps/
│   └── default.json       # Shortcut keys configuration
└── README.md              # Documentation
```

---

## Configuration & Customization

The extension operates with the following environment variables (set in your environment or Zed settings):
- `ASTRO_PROBE_URL`: The URL of the running Astro-Probe Middle Layer daemon (defaults to `http://localhost:3000`).

---

## Provided LSP Commands & Keyboard Shortcuts

The LSP adapter registers and handles the following commands:

| Command | Keyboard Shortcut | Action Description |
|---|---|---|
| `astro-probe.registerWorkspace` | *Command Palette* | Manually register the current workspace root with the Astro-Probe server. |
| `astro-probe.triggerReanalysis` | `ctrl-alt-a` | Trigger an AST re-analysis of the workspace codebase and synchronize database caches. |
| `astro-probe.openVisualizer` | `ctrl-alt-v` | Open your default browser to the web visualizer dashboard for the active workspace. |

---

## Deep-Linking (Node-to-Code Navigation)

To support clicking a node in the web-based call-graph DAG and opening that file directly in Zed, the visualizer uses the registered OS-level `zed://` protocol.

### Format
The deep-links are formatted as:
```
zed://file/<absolute_path>:<line>:<col>
```

**Example URL**:
`zed://file/D:/project/rust/astro-probe/src/Main.java:24:8`

When clicked, the OS routes the URI to the Zed executable, which focuses the editor and positions the cursor exactly at line 24, column 8. All backslashes (`\`) are normalized to forward slashes (`/`) before URL generation.
