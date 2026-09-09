# Astro-Probe Zed Extension Manual / Zed 插件使用及技术手册

[English](#english) | [中文](#中文)

---

## English

This document provides setup instructions, architectural details, workspace registration procedures, and deep-linking (jump-to-line) coordination details for the Astro-Probe Zed editor extension.

### Setup & Configurations

The Astro-Probe Zed extension is a lightweight extension that bridges the Zed editor with the Astro-Probe three-tier visualization system.

#### 1. Metadata Configuration (`extension.toml`)
The extension metadata is defined in `extension.toml`. It specifies the extension identification and registers the LSP server:
```toml
id = "astro-probe"
name = "Astro-Probe"
version = "1.0.0"
schema_version = 1
description = "Zed integration for Astro-Probe code lineage and call-graph visualization."
authors = ["Astro-Probe Team <info@astro-probe.io>"]
repository = "https://github.com/Freedomfirebody/astro-probe"

[language_servers.astro-probe]
name = "astro-probe"
languages = ["Java"]
```
* **Languages**: Matches `Java` source files to trigger the language server.
* **Language Server**: Declares `astro-probe` as the language server provider.

#### 2. LSP Adapter (`lsp-adapter.js`)
The `lsp-adapter.js` runs natively under Node.js on the host. It implements the Language Server Protocol (LSP) and translates editor events/commands into HTTP API requests for the Astro-Probe middle-layer server.
* **Host Address**: Reads `ASTRO_PROBE_URL` from environment variables, defaulting to `http://localhost:3000`.
* **Execution Environment**: Spawned by the Wasm bootstrapper wrapper (`extension.wasm`), receiving the project root path as arguments.

#### 3. Keyboard Shortcuts (`keymaps/default.json`)
Custom shortcuts are mapped to editor context commands to facilitate quick actions:
```json
[
  {
    "context": "Editor",
    "bindings": {
      "ctrl-alt-a": "astro-probe:trigger-reanalysis",
      "ctrl-alt-v": "astro-probe:open-visualizer"
    }
  }
]
```
These keys bind keyboard actions to specific LSP commands registered by `lsp-adapter.js`.

---

### Workspace Registration

To track static analysis facts, call graphs, and DFG lineage edges, workspaces must be registered with the Astro-Probe system. The Zed extension supports two modes of registration:

#### 1. Automatic Registration (On Initialization)
When you open a Java project workspace in Zed, the language server initializes. During the initialization phase (`onInitialize` event):
1. The LSP adapter retrieves the workspace root URI via `params.rootUri` or `params.rootPath`.
2. It parses and resolves the folder URI into an absolute path (e.g. `D:/project/rust/astro-probe`).
3. It sanitizes the path (removes trailing slashes) and extracts the directory name as the workspace name.
4. It issues a `POST` request to `${BACKEND_URL}/api/workspaces` with the workspace name and project path:
   ```json
   {
     "name": "astro-probe",
     "project_path": "D:/project/rust/astro-probe"
   }
   ```
5. The middle-layer server returns a JSON response containing a unique workspace ID (e.g. `1`), which the LSP adapter caches as `activeWorkspaceId`.

#### 2. Manual Registration (Via Command Palette)
If the workspace failed to register automatically (e.g., if Zed was opened without a defined root), you can trigger it manually:
1. Open the Zed Command Palette (`ctrl-shift-p` or `cmd-shift-p`).
2. Search and execute `astro-probe:register-workspace` or `astro-probe.registerWorkspace`.
3. The LSP adapter will repeat the registration handshake with the backend and show an information dialog upon success.

---

### Deep-Linking Jump-to-Line Coordination

A key feature of the Astro-Probe visualizer is the ability to navigate from a node in the call graph DAG or data-flow lineage directly to the exact line of code in the Zed editor.

#### 1. URI Schema
This feature is coordinated via the OS-registered custom protocol `zed://`. The web frontend generates a deep-link with the following format:
```
zed://file/<absolute_path>:<line>:<column>
```

#### 2. Path Normalization
Windows paths containing backslashes (e.g., `D:\project\rust\astro-probe\src\Main.java`) are normalized by replacing all backslashes (`\`) with forward slashes (`/`) to form a valid URI.
* **Example Deep-Link**:
  `zed://file/D:/project/rust/astro-probe/src/Main.java:24:8`

#### 3. Redirection Flow
1. In the frontend interactive DAG (rendered using Cytoscape.js or D3.js), clicking on a node triggers a click handler.
2. The handler requests the exact symbol location (file path, start line, start column) of the FQN from the Node.js middle-layer server.
3. The React app redirects the browser location to the generated `zed://` URI.
4. The Operating System intercepts the `zed://` protocol request and routes it to the local Zed editor executable.
5. Zed parses the file path, focuses the file, and scrolls to place the cursor precisely at the target line and column.

---

## 中文

本文档提供 Astro-Probe Zed 编辑器插件的安装配置说明、架构细节、工作区注册流程以及深层链接（跳转至代码行）的协调细节。

### 安装与配置说明

Astro-Probe Zed 插件是一个轻量级扩展，用于桥接 Zed 编辑器与 Astro-Probe 三层可视化系统。

#### 1. 元数据配置 (`extension.toml`)
插件的元数据在 `extension.toml` 中定义，负责声明插件标识并注册 LSP 服务：
```toml
id = "astro-probe"
name = "Astro-Probe"
version = "1.0.0"
schema_version = 1
description = "Zed integration for Astro-Probe code lineage and call-graph visualization."
authors = ["Astro-Probe Team <info@astro-probe.io>"]
repository = "https://github.com/Freedomfirebody/astro-probe"

[language_servers.astro-probe]
name = "astro-probe"
languages = ["Java"]
```
* **Languages**：匹配 `Java` 源码文件以触发语言服务器。
* **Language Server**：声明 `astro-probe` 作为语言服务提供者。

#### 2. LSP 适配器 (`lsp-adapter.js`)
`lsp-adapter.js` 在宿主机的 Node.js 环境下原生运行。它实现了语言服务器协议（LSP），将编辑器的事件与命令请求转化为指向 Astro-Probe 中层服务器的 HTTP API 请求。
* **服务地址**：从环境变量 `ASTRO_PROBE_URL` 读取，默认值为 `http://localhost:3000`。
* **执行环境**：由 Wasm 引导包装器（`extension.wasm`）启动，并将项目根路径作为参数传递。

#### 3. 键盘快捷键 (`keymaps/default.json`)
自定义快捷键映射到编辑器上下文命令，以实现快速操作：
```json
[
  {
    "context": "Editor",
    "bindings": {
      "ctrl-alt-a": "astro-probe:trigger-reanalysis",
      "ctrl-alt-v": "astro-probe:open-visualizer"
    }
  }
]
```
这些按键绑定了 `lsp-adapter.js` 所注册的特定 LSP 命令。

---

### 工作区注册

为了追踪静态分析数据、调用图和数据流血缘边，工作区必须在 Astro-Probe 系统中注册。Zed 插件支持两种注册模式：

#### 1. 自动注册（初始化时）
当你在 Zed 中打开 Java 项目工作区时，语言服务器将启动。在初始化阶段（`onInitialize` 事件）：
1. LSP 适配器通过 `params.rootUri` 或 `params.rootPath` 获取工作区根目录 URI。
2. 它解析该 URI 为绝对路径（例如 `D:/project/rust/astro-probe`）。
3. 净化路径（移除末尾斜杠）并提取目录名作为工作区名称。
4. 它向 `${BACKEND_URL}/api/workspaces` 发送一个 `POST` 请求，携带工作区名称与项目路径：
   ```json
   {
     "name": "astro-probe",
     "project_path": "D:/project/rust/astro-probe"
   }
   ```
5. 中层服务器返回一个包含唯一工作区 ID（如 `1`）的 JSON 响应，LSP 适配器将其缓存为 `activeWorkspaceId`。

#### 2. 手动注册（通过命令面板）
如果工作区未能自动注册成功（例如，在未定义根目录的情况下打开了 Zed），你可以手动触发：
1. 打开 Zed 命令面板（`ctrl-shift-p` 或 `cmd-shift-p`）。
2. 搜索并执行 `astro-probe:register-workspace` 或 `astro-probe.registerWorkspace`。
3. LSP 适配器将与后端重新执行注册握手，并在成功后显示信息提示框。

---

### 深层链接跳转至代码行协调

Astro-Probe 可视化器的一个关键功能是能够从调用图 DAG 或数据流血缘的节点直接跳转到 Zed 编辑器中的对应代码行。

#### 1. 统一资源标识符 (URI Schema)
该功能通过操作系统注册的自定义协议 `zed://` 进行协调。Web 前端生成如下格式的深层链接：
```
zed://file/<absolute_path>:<line>:<column>
```

#### 2. 路径标准化
包含反斜杠的 Windows 路径（例如 `D:\project\rust\astro-probe\src\Main.java`）会被标准化：所有的反斜杠（`\`）都会被替换为正斜杠（`/`），以形成合法的 URI。
* **深层链接示例**：
  `zed://file/D:/project/rust/astro-probe/src/Main.java:24:8`

#### 3. 跳转执行流
1. 在前端的交互式 DAG（通过 Cytoscape.js 或 D3.js 渲染）中，点击节点触发点击事件处理器。
2. 处理器向 Node.js 中层服务器请求该 FQN 的精确符号位置（文件路径、开始行、开始列）。
3. React 应用重定向浏览器地址到生成的 `zed://` URI。
4. 操作系统拦截 `zed://` 协议请求，并将其路由到本地的 Zed 编辑器可执行程序。
5. Zed 解析文件路径，聚焦该文件，并自动滚动、准确定位光标至目标行和列。
