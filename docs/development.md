# Astro-Probe Developer Manual 🛠️

[English](#english) | [中文](#中文)

---

## English

This developer manual provides deep technical insights into Astro-Probe's architecture, core abstractions, SQLite schema, Points-To solver, and contribution guidelines.

---

### Table of Contents
- [1. Architecture & Crate Decoupling](#1-architecture--crate-decoupling)
- [2. Database Schema Reference](#2-database-schema-reference)
- [3. Points-To Solver & Call Graph Construction](#3-points-to-solver--call-graph-construction)
- [4. Data Flow Graph (DFG) & Incremental Analysis](#4-data-flow-graph-dfg--incremental-analysis)
- [5. Adding Support for a New Language Frontend](#5-adding-support-for-a-new-language-frontend)
- [6. Development Guidelines & Quality Gates](#6-development-guidelines--quality-gates)

---

### 1. Architecture & Crate Decoupling

Astro-Probe is designed with a **language-agnostic core** (`astro-probe-core`). Crate dependencies strictly flow in a single direction. Java-specific syntax, parsing, and bytecode logic are completely contained in `astro-probe-java`.

#### Core Trait Abstractions (`crates/astro-probe-core/src/traits.rs`)
* `SourceParser`: Translates raw source files into relational Facts (classes, methods, assignments).
* `DependencyAnalyzer`: Parses dependencies (e.g., Maven JARs, npm node_modules) to resolve external method signatures and points-to summaries.
* `FrameworkAnalyzer`: Pluggable hooks for framework-specific behaviors (e.g., Spring DI autowire rules, Spring AOP proxy matching).
* `LanguageFrontend`: The registrar that aggregates the above interfaces and exposes them to the `WorkspaceManager`.

---

### 2. Database Schema Reference

SQLite stores workspace and incremental caches. The SQL schema is defined in `crates/astro-probe-db/src/lib.rs`.

#### Key Database Tables
* `source_assignments`: Stores assignments, copies, and field allocations. `assignment_type` values are `ALLOC`, `COPY`, `FIELD_READ`, and `FIELD_WRITE`.
* `points_to_sets`: Pointers mapping table. Tracks `variable_fqn` to its set of `alloc_id` allocations under distinct `context` execution paths.
* `call_edges`: Call graph edges tracking caller and callee FQNs alongside their context environments (`caller_context`, `callee_context`).
* `file_hashes`: Content MD5 hashes of source files, used by the parser to skip unchanged files during re-analysis.
* `method_summaries`: Pre-computed points-to summaries of third-party dependencies, resolving complex cross-module targets quickly.

---

### 3. Points-To Solver & Call Graph Construction

The core solver (`crates/astro-probe-core/src/cg.rs`) implements a field-sensitive Andersen Points-To analysis.

#### 3.1 1-CFA Context Sensitivity & Dynamic Bypass
* **Context Propagation**: The solver propagates calling contexts (up to 1 call level depth) to resolve dynamic dispatches in virtual methods (e.g., Strategy DI patterns).
* **Dynamic CFA Bypass (`_disable_cfa = call_sites.len() > 1000`)**: On large projects (such as Nacos with thousands of call sites), propagating context causes state space explosion. When the threshold is exceeded, the solver automatically falls back to 0-CFA context-insensitive mode, ensuring fast solver convergence.

#### 3.2 Incremental Solver Strategy
Rather than purging tables and solving from scratch on file changes, the solver is fully incremental:
1. **Load Cache**: On starting an incremental re-analysis, it loads existing `points_to_sets` and `call_edges` from the database.
2. **Delta Updates**: Java frontend parses only changed files, deleting old assignments and local facts for the dirty classes.
3. **Commit Deltas**: The solver iterates to a fixpoint, and checks its database write cache (`loaded_pts` and `loaded_edges`) to only insert newly discovered facts, eliminating database write latency and massive WAL overheads.

---

### 4. Data Flow Graph (DFG) & Incremental Analysis

The data flow engine (`crates/astro-probe-core/src/dfg.rs`):
* Utilizes Call Graph edges to trace variable propagation paths across method bounds.
* **Transitive Reduction**: Bypasses simple assignment chains (`a = b = c = d`) and only retains endpoints, preventing memory and database size bloat.

---

### 5. Adding Support for a New Language Frontend

To support another language (e.g. Go, Python, C#):
1. **Create Crate**: Add a new crate under `crates/` (e.g., `astro-probe-go`) and register it in the root `Cargo.toml`.
2. **Implement Traits**: Implement `SourceParser`, `DependencyAnalyzer` (optional), and `LanguageFrontend`.
3. **Register Frontend**: Open `crates/astro-probe-server/src/kernel/manager.rs` and register your frontend inside `WorkspaceManager::new()`:
   ```rust
   let mut frontend_manager = FrontendManager::new();
   frontend_manager.register("go", Box::new(GoFrontend::new()));
   ```

---

### 6. Development Guidelines & Quality Gates

Our codebase adheres to a rigorous double sign-off governance pipeline before commits are allowed on `dev`:

```
Development ──> Cargo Clippy ──> Cargo Test ──> Code Auditor (PASS) ──> Project Evaluator (PASS) ──> Git Commit & Tag
```

#### Local Quality Tasks
* **Clippy lint check**: Run `cargo clippy --workspace --all-targets -- -D warnings`. Warnings are treated as compile errors.
* **Workspace test suite**: Run `cargo test --workspace` and verify that both unit and performance benchmark tests pass cleanly.

---

---

## 中文

本手册专为引擎开发者编写，详细介绍 Astro-Probe 的内部架构、Trait 设计、增量算法以及如何扩展支持新的分析语言。

---

### 目录
- [1. 核心设计原则与解耦](#1-核心设计原则与解耦-1)
- [2. 数据库 Schema 架构](#2-数据库-schema-架构-1)
- [3. Points-To 求解器与调用图算法](#3-points-to-求解器与调用图算法-1)
- [4. 数据流图与增量更新机制](#4-数据流图与增量更新机制-1)
- [5. 扩展指南：如何开发新语言前端](#5-扩展指南如何开发新语言前端-1)
- [6. 项目测试与质量门禁规范](#6-项目测试与质量门禁规范-1)

---

### 1. 核心设计原则与解耦

Astro-Probe 遵循**语言无关核心**的设计思想。所有与具体语言（如 Java 语法、Spring 注解）相关的逻辑，必须完全封装在特定前端 crate 中。

#### 核心抽象 Trait (`crates/astro-probe-core/src/traits.rs`)
- `SourceParser`：将源码解析为归一化的结构 Facts（类定义、方法声明、赋值语句等）。
- `DependencyAnalyzer`：解析依赖（如 Maven 的 JAR、npm 的 node_modules）并提取方法签名与调用 Facts。
- `FrameworkAnalyzer`：用于识别特定框架结构（如 Spring AOP 代理、IOC 注入模式），以分析事实微调 Points-To 约束。
- `LanguageFrontend`：充当各语言能力的注册器，与 `WorkspaceManager` 对接。

通过这种设计，`astro-probe-core` 没有任何 Java / Spring 相关的依赖项，仅依靠标准数据结构与 DDL Facts 进行通用分析。

---

### 2. 数据库 Schema 架构

Astro-Probe 借助 SQLite 作为跨阶段的事实（Facts）存储和增量缓存。核心表定义位于 `crates/astro-probe-db/src/lib.rs` 中：

#### 核心表结构及说明
* `source_assignments`：存储赋值、拷贝、字段读写、以及对象分配 Facts。
  - `assignment_type` 包括：`ALLOC` (对象分配), `COPY` (变量拷贝), `FIELD_READ` (字段读), `FIELD_WRITE` (字段写)。
* `points_to_sets`：指针分析结果表，记录每个变量在特定调用上下文下的所有可能分配点。
  - 字段包括：`variable_fqn`, `alloc_id`, `context` (变量所属上下文), `alloc_context` (分配所属上下文)。
* `call_edges`：上下文敏感的调用图边。
  - 字段包括：`caller`, `callee`, `caller_context`, `callee_context`, `is_virtual` (是否为虚方法调用)。
* `file_hashes`：用于增量更新的源码文件内容 MD5 哈希表。
* `method_summaries`：缓存依赖包（如 JAR）中方法的指针行为概要，提高跨包分析效率。

---

### 3. Points-To 求解器与调用图算法

Astro-Probe 实现了一个高效的 **Andersen 字段敏感（Field-Sensitive）** 求解器，位于 `crates/astro-probe-core/src/cg.rs`。

#### 3.1 1-CFA 上下文敏感与动态旁路机制
- **上下文传播**：标准的 1-CFA 算法将调用点 `call_id` 作为被调用方法的上下文，有效区分不同调用流下的虛方法派发（如 Strategy 模式）。
- **动态深度限制**：为防止长调用链导致上下文爆炸，在传播时通过 `C_caller` 状态进行限制，实现至多 1 层的上下文传播深度，多层时自动重置为全局上下文 `""`。
- **大项目旁路 (CFA Bypass)**：当全局解析出的调用点数量大于 1000（如 Nacos 项目拥有上万调用点）时，将自动开启旁路开关（`_disable_cfa = true`），退回到上下文无关（0-CFA）以获得超过 10 倍的求解速度，确保指针分析在大型工程中极速收敛。

#### 3.2 增量更新与局部覆盖
在执行文件级增量分析时，Points-To 求解器表现出优异的增量保存特性：
1. **加载缓存**：在启动阶段，从 `points_to_sets` 和 `call_edges` 读取已有指针集合与调用关系。
2. **增量删除**：当检测到源文件被修改时，Java 前端会在解析阶段仅删除受污染类（Dirty Classes）的 assignments Facts 以及对应的旧指针记录。
3. **只增不改**：求解器在 fixpoint 收敛后，利用哈希集合（`loaded_pts` 和 `loaded_edges`）过滤掉已经在数据库中存在的记录，只对**新派生出的变化差量**进行 SQL batch 插入，显著降低了 WAL 大文件的写入及锁冲突开销。

---

### 4. 数据流图与增量更新机制

数据流分析位于 `crates/astro-probe-core/src/dfg.rs`：
* 它使用指针分析计算的 Call Graph，在整个系统的变量节点之间构建有向数据依赖图。
* **传递性规约 (Transitive Reduction)**：为避免中间变量链过长造成内存膨胀，数据流构建时会对简单的传递分配链进行规约，仅保留核心节点到目标节点的流向关系。

---

### 5. 扩展指南：如何开发新语言前端

如需为 Astro-Probe 增加一门新语言（例如 Go 或 Go/Spring 等效框架），请按照以下步骤开发：

1. **新建前端 Crate**：在 `crates/` 下新建 `astro-probe-go` crate，并在工作区 `Cargo.toml` 中注册。
2. **实现核心 Trait**：
   - 实现 `SourceParser`：使用 Go 的 AST 解析器（或 `tree-sitter`）遍历代码，提取类、结构体、赋值和调用 Facts。
   - 实现 `FrameworkAnalyzer`（可选）：用于匹配诸如 Go Wire / Dig 依赖注入或路由注册模式。
   - 实现 `LanguageFrontend`。
3. **注册前端**：在 `astro-probe-server` 的 `crates/astro-probe-server/src/kernel/manager.rs` 的 `WorkspaceManager::new()` 中注册您的 Go 前端实现：
   ```rust
   let mut frontend_manager = FrontendManager::new();
   frontend_manager.register("go", Box::new(GoFrontend::new()));
   ```

---

### 6. 项目测试与质量门禁规范

我们在项目迭代中遵循极其严苛的双重签核开发流程：

```
开发模块 ──> Clippy 检查通过 ──> 单元/集成测试通过 ──> 🔍代码审计员报告 (PASS) ──> 📊项目评估员报告 (PASS) ──> Git Commit & Tag
```

#### 开发要求
* **Clippy 无警告**：提交前必须保证 `cargo clippy --workspace --all-targets -- -D warnings` 无错误与警告。
* **双重治理报告**：每个 Milestone 开发完毕后，需自动生成或手动整理 `artifacts/audit_report_M{N}.md`（零阻塞问题）和 `artifacts/evaluation_report_M{N}.md`（偏离度极低且需求覆盖 100%）。
* **防止回归**：每次代码修改都必须重新跑全套集成测试以防止逻辑回归。
