# Astro-Probe 🌌

[English](#english) | [中文](#中文)

---

## English

Astro-Probe is a production-grade **multi-language static code analysis engine** written in Rust. It features a language-agnostic analysis core (Points-To solving, data-flow analysis, context-sensitive call graph construction), with Java (supporting the Spring framework and Maven dependency resolution) as the first frontend implementation.

The project utilizes a multi-crate workspace architecture following strict dependency decoupling, designed for horizontal extensibility and optimal analysis runtime.

---

### Table of Contents
- [Project Architecture](#project-architecture)
- [Key Features](#key-features)
- [Quick Start](#quick-start)
- [Testing & Verification](#testing--verification)
- [Documentation Links](#documentation-links)
- [License](#license)

---

### Project Architecture

Astro-Probe follows strict one-way dependency rules, organized into 5 focused Cargo crates and a web visualization system:

```
astro-probe (Workspace Root)
├── crates/
│   ├── astro-probe-core/         # Language-agnostic core (Points-To & DFG engine)
│   ├── astro-probe-db/           # Database layer (SQLite & r2d2 connection pool)
│   ├── astro-probe-java/         # Java frontend (Bytecode analyzer, Maven resolver, Spring mapping)
│   ├── astro-probe-server/       # Service layer (CLI, REST APIs, and MCP SSE server)
│   └── astro-probe-tests/        # Integration tests (Milestone validation & Nacos stress tests)
├── test-samples/                 # Test project samples
└── visualizers/                  # Three-tier visualization and integration system
    ├── server/                   # Node.js middle-layer Express server & symbol resolver
    ├── frontend/                 # Vite/React SPA dashboard & DAG visualizer
    ├── zed-extension/            # Zed lightweight editor plugin for workspace registration & commands
    └── e2e-tests/                # End-to-end integration and verification test suite
```

#### Three-Tier Visualization Architecture
Astro-Probe features a three-tier visualization system to interactively explore call graphs, data-flow lineage, and Spring routing paths:
1. **Bottom Layer (Rust Core & Daemon)**: The high-performance Rust static analysis engine (`astro-probe-server`) exposing SQLite-backed REST APIs.
2. **Middle Layer (Node.js Helper Server)**: Located in `visualizers/server`, this acts as a business logic hub. It proxies commands/graph queries to the Rust backend and runs an AST-based Java parser/symbol resolver to find exact source lines/columns for FQNs.
3. **Frontend Layer (React Frontend)**: Located in `visualizers/frontend`, a Vite + React Single-Page App utilizing Cytoscape.js/D3.js for interactive DAG visualizers, Monaco Editor for in-context code preview, and a dashboard for workspace management.

There is also editor integration:
* **Zed Extension**: Located in `visualizers/zed-extension`, it provides workspace registration, manual re-analysis commands, and deep-linking jump-to-line coordination.

#### Crate Dependency Direction
```
[astro-probe-server] ──> [astro-probe-java] ──> [astro-probe-core] ──> [astro-probe-db]
```

---

### Key Features

1. **Andersen Points-To Analysis**: Field-sensitive pointers analysis for precise target tracking.
2. **k-CFA Context-Sensitive Call Graph**: Supports 1-CFA call context sensitivity with dynamic context bypass (automatically falls back to 0-CFA on large codebases to balance speed and accuracy).
3. **Incremental Analysis**: Content hash tracking for incremental source parsing, and database-backed incremental Points-To and Call Graph solving. For Nacos (2280+ Java files), incremental re-analysis finishes in **under 15 seconds**.
4. **Build Tool & Framework Integration**:
   - **Maven Integration**: Automatically parses `pom.xml` configurations, resolves parent dependencies, version overrides, and caches local `.m2/repository` JARs.
   - **Spring Ecosystem Support**: Resolves Spring DI injection, Spring MVC route mappings, Spring AOP advice pointcuts, Spring event publishers (`publishEvent` to `@EventListener`), and `@Async` flow lineage.
5. **Multi-Protocol Interface**: Exposes standard RESTful HTTP APIs and **MCP (Model Context Protocol)** to connect static analysis directly into external client workflows and tools.

---

### Quick Start

#### Prerequisites
- Rust (1.75+) and Cargo compiler toolchain.
- Java & Maven configured (for Java analysis).
- Node.js (18+) and npm package manager (for visualizer components).

#### Build Project
Compile the workspace in release mode:
```bash
cargo build --release
```

#### Run Server
Launch the HTTP/MCP server (listens on port `8080` by default):
```bash
./target/release/astro-probe-server
```

#### Start Visualizer Development Servers
To start the visualizer in development mode:
1. Bootstrap the components (installs dependencies in `server`, `frontend`, and `zed-extension`):
   ```bash
   cd visualizers
   npm install
   npm run bootstrap
   ```
2. Start the development servers (concurrently runs both Node.js server and Vite React frontend):
   ```bash
   npm run dev
   ```
   * The backend helper server runs on `http://localhost:3000`.
   * The frontend web portal runs on `http://localhost:5173`.

---

### Testing & Verification

Astro-Probe includes a comprehensive suite of unit tests, integration tests, performance benchmarks, and end-to-end integration tests.

#### Run Full Test Suite
```bash
cargo test --workspace
```

#### Run Nacos Performance Benchmarks
```bash
cargo test -p astro-probe-tests --test perf_benchmark test_perf_benchmark_nacos --release -- --nocapture
```

#### Run Visualizer E2E Tests
We provide a comprehensive End-to-End integration test suite under `visualizers/e2e-tests`. You can bootstrap, build production assets, and run the E2E test suite automatically via the PowerShell script:
```powershell
./visualizers/run-e2e.ps1
```
Or manually run:
```bash
cd visualizers/e2e-tests
npm install
node test-runner.js
```

---

### Documentation Links
* 📘 **[User Manual (English)](./docs/usage.md)**
* 🛠️ **[Developer Manual (English)](./docs/development.md)**
* 🔌 **[Zed Extension Manual (English/Chinese)](./docs/zed-plugin.md)**

---

### License

This project is licensed under the [Apache-2.0 License](LICENSE).

---

## 中文

Astro-Probe 是一个采用 Rust 编写的、生产级**多语言静态代码分析引擎**。它拥有语言无关的分析核心（Points-To 求解、数据流分析、上下文敏感调用图构建），并以 Java（支持 Spring 框架和 Maven 依赖解析）作为首个前端语言实现。

项目采用了多 crate 工作区架构，遵循严格的依赖解耦设计，具备极佳的水平扩展性与分析效率。

---

### 目录
- [项目架构](#项目架构-1)
- [核心特性](#核心特性-1)
- [快速入门](#快速入门-1)
- [测试与验证](#测试与验证-1)
- [开发与使用手册](#开发与使用手册)
- [开源协议](#开源协议-1)

---

### 项目架构

Astro-Probe 遵循严格的单向依赖准则，由 5 个聚焦特定功能的 crate 以及三层可视化系统组成：

```
astro-probe (工作区根目录)
├── crates/
│   ├── astro-probe-core/         # 核心抽象层 (语言无关，Points-To 与数据流分析引擎)
│   ├── astro-probe-db/           # 数据库存储层 (基于 SQLite 与 r2d2 连接池)
│   ├── astro-probe-java/         # Java 语言前端 (字节码解析、Maven 解析、Spring 框架分析)
│   ├── astro-probe-server/       # 对外服务层 (提供 CLI、REST API 以及 MCP SSE 接口)
│   └── astro-probe-tests/        # 集成测试 crate (全自动 Milestone 测试集与 Nacos 压力测试)
├── test-samples/                 # 测试样例项目
└── visualizers/                  # 三层可视化与集成系统
    ├── server/                   # Node.js 中层 Express 服务器与符号解析器
    ├── frontend/                 # Vite/React 单页 Web 应用 (仪表盘与 DAG 可视化)
    ├── zed-extension/            # Zed 轻量级编辑器插件 (工作区注册与命令交互)
    └── e2e-tests/                # 端到端集成与验证测试套件
```

#### 三层可视化架构
Astro-Probe 包含一个三层架构的可视化系统，用于交互式展示调用图、数据流血缘以及 Spring 路由路径：
1. **底层（Rust 核心与守护进程）**：由高性能 Rust 静态分析引擎（`astro-probe-server`）提供基于 SQLite 缓存的 REST API 接口。
2. **中层（Node.js 辅助服务器）**：位于 `visualizers/server`，作为业务融合枢纽。它向 Rust 后端代理命令和图查询，并运行基于 AST 的 Java 解析器与符号解析器，根据 FQN 定位精确的源码行号与列号。
3. **前端层（React 前端）**：位于 `visualizers/frontend`，采用 Vite + React 构建的单页 Web 应用。利用 Cytoscape.js/D3.js 提供交互式 DAG 可视化界面，使用 Monaco Editor 实现源码预览，并提供工作区管理仪表盘。

此外，项目还集成了编辑器插件：
* **Zed 插件**：位于 `visualizers/zed-extension`，提供自动/手动注册工作区、手动触发重新分析命令，以及深层链接（deep-linking）跳转至对应代码行的协调能力。

#### 依赖关系图
```
[astro-probe-server] ──> [astro-probe-java] ──> [astro-probe-core] ──> [astro-probe-db]
```

---

### 核心特性

1. **Andersen 指针分析 (Points-To Analysis)**：实现字段敏感（Field-Sensitive）的指针分析。
2. **k-CFA 上下文敏感调用图**：支持 1-CFA 调用上下文敏感度分析，并实现动态上下文旁路（针对大规模代码自动回退至 0-CFA）以平衡分析精度与执行效率。
3. **高效增量分析**：基于文件内容哈希进行增量解析，并实现数据库缓存级别的指针与调用边增量分析。对于 2280+ 个 Java 文件的 Nacos 项目，增量再分析可在 **15 秒内**完成。
4. **框架与依赖智能解析**：
   - **Maven 构建集成**：自动解析 `pom.xml`，补全第三方依赖 JAR 包（支持继承与版本冲突调解）。
   - **Spring 生态解析**：支持 Spring DI 依赖注入、Spring MVC 路由映射、Spring AOP 切面通知匹配、Spring Event 异步事件传递与 `@Async` 调用链分析。
5. **多协议对外服务**：支持标准的 HTTP API 以及 **MCP (Model Context Protocol)**，方便对接外部集成插件与自动化工具链。

---

### 快速入门

#### 前提条件
- 安装 Rust (1.75+) 及 Cargo 编译环境。
- 配置好 Java/Maven 环境（如需分析 Java 项目）。
- 安装 Node.js (18+) 及 npm 包管理器（以运行可视化组件）。

#### 编译项目
在工作区根目录下执行 release 编译：
```bash
cargo build --release
```

#### 运行服务
启动 `astro-probe-server`（默认监听 HTTP 8080 端口）：
```bash
./target/release/astro-probe-server
```

#### 运行可视化开发服务器
以开发模式启动可视化服务：
1. 引导安装依赖（在 `server`、`frontend` 及 `zed-extension` 目录中安装所需依赖）：
   ```bash
   cd visualizers
   npm install
   npm run bootstrap
   ```
2. 启动开发服务器（同时并发运行 Node.js 服务与 Vite React 前端）：
   ```bash
   npm run dev
   ```
   * 后端辅助服务器默认运行在 `http://localhost:3000`。
   * 前端 Web 门户默认运行在 `http://localhost:5173`。

---

### 测试与验证

项目提供全套单元测试与集成测试，并包含对真实世界大型项目（Nacos）的性能回归压力测试以及端到端（E2E）集成测试。

#### 运行完整测试套件
```bash
cargo test --workspace
```

#### 运行 Nacos 性能压力基准测试
```bash
cargo test -p astro-probe-tests --test perf_benchmark test_perf_benchmark_nacos --release -- --nocapture
```

#### 运行可视化 E2E 测试
我们在 `visualizers/e2e-tests` 下提供了一套完整的端到端集成测试集。你可以直接运行 PowerShell 脚本来自动引导依赖、构建前端生产环境产物并执行 E2E 测试：
```powershell
./visualizers/run-e2e.ps1
```
或者手动执行以下命令：
```bash
cd visualizers/e2e-tests
npm install
node test-runner.js
```

---

### 开发与使用手册

为了帮助您更深入地使用或参与开发，我们准备了以下详细手册：

* 📘 **[使用手册 (Usage Manual)](./docs/usage.md)**：包含如何创建工作区、执行静态分析、调用 REST API 以及配置 MCP 服务连接。
* 🛠️ **[开发手册 (Development Manual)](./docs/development.md)**：包含核心 Trait 设计、表 Schema 定义、增量机制说明以及新增语言前端指南。
* 🔌 **[Zed 插件使用手册 (Zed Extension Manual)](./docs/zed-plugin.md)**：包含插件配置、快捷键说明、工作区自动/手动注册及深层链接跳转说明。

---

### 开源协议

本项目采用 [Apache-2.0](LICENSE) 协议开源。