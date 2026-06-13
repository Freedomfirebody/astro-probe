# Astro-Probe User Manual 📘

[English](#english) | [中文](#中文)

---

## English

Astro-Probe provides a Command Line Interface (CLI), a RESTful HTTP API, and a Model Context Protocol (MCP) server integration. This manual describes how to configure, run, and query Astro-Probe for static code analysis.

---

### Table of Contents
- [1. Command Line Interface (CLI)](#1-command-line-interface-cli)
- [2. RESTful HTTP API Reference](#2-restful-http-api-reference)
- [3. MCP (Model Context Protocol) Integration](#3-mcp-model-context-protocol-integration)
- [4. Spring Analysis Workflows](#4-spring-analysis-workflows)

---

### 1. Command Line Interface (CLI)

The compiled binary `astro-probe-server` acts as the daemon and entrypoint.

#### Start Server
By default, running the server starts the daemon listening on port `8080`:
```bash
./target/release/astro-probe-server
```

#### Command Line Arguments
* `--port <PORT>`: Port for the HTTP API (default: 8080).
* `--db <PATH>`: Custom path to the global SQLite database cache.
* `--mcp-transport <MODE>`: Select the transport layer for the MCP protocol, either `sse` (Server-Sent Events, default) or `stdio`.

---

### 2. RESTful HTTP API Reference

All analysis results are persisted in a workspace-specific SQLite database. You can interact with the daemon using the following REST endpoints.

#### 2.1 Create Workspace and Analyze Project
* **Method**: `POST`
* **URL**: `/api/workspaces`
* **Headers**: `Content-Type: application/json`
* **Request Body**:
  ```json
  {
    "name": "my-spring-project",
    "project_path": "/path/to/your/java/project"
  }
  ```
* **Response (Status 201 Created)**:
  ```json
  {
    "id": "e9d8924b-01fc-4bfb-927a-8ef7d780fb92",
    "name": "my-spring-project",
    "project_path": "/path/to/your/java/project",
    "status": "Ready"
  }
  ```
* **Description**:
  Triggers a full workspace analysis:
  1. Resolves Maven `pom.xml` configurations, inherits dependency versions, and loads local `.m2/repository` JARs.
  2. Parses Java sources into relational Facts inside the database.
  3. Computes 1-CFA context-sensitive Andersen Points-To sets and builds the Call Graph.
  4. Generates the Data Flow Graph (DFG).

#### 2.2 Query Data Flow Lineage
* **Method**: `GET`
* **URL**: `/api/workspaces/:id/lineage`
* **Query Parameters**:
  - `node`: The target variable/parameter/return FQN or its suffix. Suffix matching is fully supported:
    - **FQN with Variable** (e.g. `Class.method#paramName` or `method#p0`): Matches parameter or local variable by method name suffix and parameter name/index.
    - **FQN with Return** (e.g. `Class.method#return`): Matches return values of the method.
    - **Simple Variable Suffix** (e.g. `input` or `p0`): Matches any variables ending with the specified name.
  - `direction`: Graph traversal direction: `upstream` (trace where data comes from) or `downstream` (trace where data flows to).
* **Response (Status 200 OK)**:
  ```json
  {
    "nodes": [
      "com.test.Client.main()#input",
      "input",
      "com.test.Service.process(java.lang.String)#p0",
      "p0"
    ],
    "edges": [
      {
        "from": "com.test.Client.main()#input",
        "to": "com.test.Service.process(java.lang.String)#p0",
        "type": "PASS_ARG"
      }
    ]
  }
  ```

#### 2.3 Query Call Graph
* **Method**: `GET`
* **URL**: `/api/workspaces/:id/call-graph`
* **Query Parameters**:
  - `method`: The caller or callee method signature or suffix. Flexible suffix matching is supported:
    - **No Parentheses** (e.g. `environmentPrepared` or `LoggingApplicationListener.environmentPrepared`): Matches any method FQN ending with the query followed by `(`.
    - **Empty Parentheses `()`** (e.g. `method()`): Matches zero-argument methods ending with `method()`.
    - **With Parameters `(...)`** (e.g. `method(int)`): Matches the exact parameter signature ending with `method(int)`.
  - `direction`: Call graph traversal direction: `incoming` (find callers) or `outgoing` (find callees).
* **Response (Status 200 OK)**:
  ```json
  {
    "edges": [
      {
        "caller": "com.test.Client.main()",
        "callee": "com.test.Service.process(java.lang.String)",
        "is_virtual": false
      }
    ]
  }
  ```

#### 2.4 Query Web Routes
* **Method**: `GET`
* **URL**: `/api/workspaces/:id/routes`
* **Query Parameters** (All optional):
  - `path`: Filter by HTTP path prefix (e.g., `/api`).
  - `http_method`: Filter by HTTP method (e.g., `GET`, `POST`).
* **Response (Status 200 OK)**:
  ```json
  {
    "routes": [
      {
        "http_method": "GET",
        "path": "/api/users",
        "controller_method_fqn": "com.example.controller.UserController.listUsers()"
      }
    ]
  }
  ```

#### 2.5 Delete Workspace
* **Method**: `DELETE`
* **URL**: `/api/workspaces/:id`
* **Response (Status 200 OK)**:
  ```json
  "Workspace deleted successfully"
  ```


---

### 3. MCP (Model Context Protocol) Integration

Astro-Probe supports integration with external clients (e.g. developer editors or command runners) via the Model Context Protocol (MCP).

#### 3.1 Client Configuration Example
Add the following connection details to your client configuration file:

```json
{
  "mcpServers": {
    "astro-probe": {
      "command": "/path/to/astro-probe/target/release/astro-probe-server",
      "args": ["--mcp-transport", "stdio"],
      "env": {}
    }
  }
}
```

#### 3.2 Exposed MCP Tools
* `create_workspace`: Create workspace and analyze project path.
* `query_lineage`: Query global data flow lineage between variable nodes.
* `list_routes`: List Spring MVC routes mapped to Controller methods.

---

### 4. Spring Analysis Workflows

Astro-Probe is optimized for Spring framework patterns:

#### 4.1 Web Route Tracing
It parses `@RestController`, `@RequestMapping`, and `@GetMapping` to populate the `web_routes` table, allowing developers and agents to trace from HTTP entry points down to database persistence queries.

#### 4.2 Event Publisher Lineage
It tracks Spring `ApplicationEventPublisher.publishEvent()` calls and resolves their dispatch destinations by matching event allocation types with receivers annotated with `@EventListener`, creating event propagation edges in the Call Graph.

#### 4.3 Collection Type Propagation
It virtualizes generic collections (like `List<T>`, `Map<K, V>`) as fields (`[element]`, `[key]`, `[value]`) to ensure points-to sets propagate accurately across collection boundaries.

---

---

## 中文

Astro-Probe 提供命令行界面（CLI）、RESTful HTTP API 以及 MCP（Model Context Protocol）服务协议。本手册将详细介绍如何配置、启动和使用这些接口进行静态代码分析。

---

### 目录
- [1. 命令行接口 (CLI) 使用](#1-命令行接口-cli-使用-1)
- [2. RESTful HTTP API 接口说明](#2-restful-http-api-接口说明-1)
- [3. MCP (Model Context Protocol) 整合](#3-mcp-model-context-protocol-整合-1)
- [4. Spring 应用分析实践](#4-spring-应用分析实践-1)

---

### 1. 命令行接口 (CLI) 使用

Astro-Probe 编译出的二进制程序 `astro-probe-server` 支持通过命令行参数直接管理和执行分析任务任务。

#### 启动服务
默认情况下，直接运行程序会启动守护进程并监听 `8080` 端口：
```bash
./target/release/astro-probe-server
```

#### 命令行参数说明
* `--port <PORT>`：指定 HTTP 服务监听的端口（默认：8080）。
* `--db <PATH>`：指定全局 SQLite 缓存数据库路径。
* `--mcp-transport <MODE>`：指定 MCP 服务传输模式，支持 `sse`（Server-Sent Events，默认）或 `stdio`。

---

### 2. RESTful HTTP API 接口说明

服务启动后，可以通过标准 HTTP 请求进行交互。分析结果持久化在工作区专属的 SQLite 数据库中。

#### 2.1 创建工作区并分析项目
* **请求方法**：`POST`
* **URL**：`/api/workspaces`
* **Header**：`Content-Type: application/json`
* **请求体**：
  ```json
  {
    "name": "my-spring-project",
    "project_path": "/path/to/your/java/project"
  }
  ```
* **响应体 (Status 201 Created)**：
  ```json
  {
    "id": "e9d8924b-01fc-4bfb-927a-8ef7d780fb92",
    "name": "my-spring-project",
    "project_path": "/path/to/your/java/project",
    "status": "Ready"
  }
  ```
* **工作流说明**：
  收到请求后，引擎将自动执行以下步骤：
  1. 解析 Maven `pom.xml` 依赖，继承版本并加载本地仓库（`.m2/repository`）的依赖 JAR 包。
  2. 提取 Java 文件为归一化分析 Facts 并写入数据库。
  3. 执行 1-CFA 上下文敏感的 Points-To 指针分析并构建调用图（Call Graph）。
  4. 构建数据流图（Data Flow Graph）并写入数据库。

#### 2.2 查询调用链与数据流向 (Lineage)
* **请求方法**：`GET`
* **URL**：`/api/workspaces/:id/lineage`
* **查询参数**：
  - `node`：目标变量、参数或返回值的全限定名（FQN）或其后缀。支持灵活的后缀匹配：
    - **方法名后缀+变量名**（例如：`Class.method#paramName` 或 `method#p0`）：按方法名后缀与参数名/参数位置进行匹配。
    - **方法名后缀+返回值**（例如：`Class.method#return`）：匹配对应方法的返回值。
    - **纯局部变量/参数名**（例如：`input` 或 `p0`）：匹配任何以其作为变量名称结尾的节点。
  - `direction`：查询方向。支持 `upstream`（逆流追溯数据来源）和 `downstream`（顺流追踪数据去向）。
* **响应体 (Status 200 OK)**：
  ```json
  {
    "nodes": [
      "com.test.Client.main()#input",
      "input",
      "com.test.Service.process(java.lang.String)#p0",
      "p0"
    ],
    "edges": [
      {
        "from": "com.test.Client.main()#input",
        "to": "com.test.Service.process(java.lang.String)#p0",
        "type": "PASS_ARG"
      }
    ]
  }
  ```

#### 2.3 查询方法调用图 (Call Graph)
* **请求方法**：`GET`
* **URL**：`/api/workspaces/:id/call-graph`
* **查询参数**：
  - `method`：方法签名或后缀名称。支持灵活的后缀匹配机制：
    - **不带括号**（如 `environmentPrepared` 或 `LoggingApplicationListener.environmentPrepared`）：匹配任何以此方法名结尾且紧跟 `(` 的方法。
    - **空括号 `()`**（如 `method()`）：匹配无参方法，筛选以 `method()` 结尾的签名。
    - **带具体参数 `(...)`**（如 `method(int)`）：精确匹配指定参数签名的后缀。
  - `direction`：查询方向。支持 `incoming`（查询谁调用了当前方法）和 `outgoing`（查询当前方法调用了谁）。
* **响应体 (Status 200 OK)**：
  ```json
  {
    "edges": [
      {
        "caller": "com.test.Client.main()",
        "callee": "com.test.Service.process(java.lang.String)",
        "is_virtual": false
      }
    ]
  }
  ```

#### 2.4 查询 Web 路由映射 (Web Routes)
* **请求方法**：`GET`
* **URL**：`/api/workspaces/:id/routes`
* **查询参数**（均为可选）：
  - `path`：按 HTTP 路径前缀过滤（如 `/api`）。
  - `http_method`：按 HTTP 方法过滤（如 `GET`, `POST`）。
* **响应体 (Status 200 OK)**：
  ```json
  {
    "routes": [
      {
        "http_method": "GET",
        "path": "/api/users",
        "controller_method_fqn": "com.example.controller.UserController.listUsers()"
      }
    ]
  }
  ```

#### 2.5 删除工作区
* **请求方法**：`DELETE`
* **URL**：`/api/workspaces/:id`
* **响应体 (Status 200 OK)**：
  ```json
  "Workspace deleted successfully"
  ```


---

### 3. MCP (Model Context Protocol) 整合

Astro-Probe 支持 Model Context Protocol 服务协议，可作为外部集成插件或命令行微服务工具链，协助外部客户端自动浏览和解释代码。

#### 3.1 接入客户端配置文件示例
您可以将 Astro-Probe 作为 `stdio` 工具接入您的客户端中。配置文件配置示例如下：

```json
{
  "mcpServers": {
    "astro-probe": {
      "command": "/path/to/astro-probe/target/release/astro-probe-server",
      "args": ["--mcp-transport", "stdio"],
      "env": {}
    }
  }
}
```

#### 3.2 暴露的 MCP Tools
大模型在连接后可以直接调用以下核心工具：
1. `create_workspace`：传入项目路径，一键扫描分析。
2. `query_lineage`：直接传入变量 FQN，查询大跨度的方法调用与数据血缘。
3. `list_routes`：列出 Spring MVC 的全部 Web 路由端点与其映射的 Controller 方法。

---

### 4. Spring 应用分析实践

Astro-Probe 针对 Spring 企业级开发模式进行了深度定制分析：

#### 4.1 Web 路由追踪
引擎能够解析 `@RestController`, `@RequestMapping`, `@GetMapping` 等注解，在数据库中建立 `web_routes` 表。
您可以通过查询数据库或通过 REST API 列出所有接口映射，快速找到 HTTP 请求入口到业务逻辑（Service）再到数据访问（DAO/Repository）的完整可达性分析。

#### 4.2 异步事件追踪
在 Spring 项目中，事件发布与监听是常见的数据流阻断点：
```java
eventPublisher.publishEvent(new OrderCreatedEvent(order));
```
Astro-Probe 的 AOP/Spring 事件扩展能够捕获 `publishEvent` 调用，根据指针分析得到的事件实际分配类型（Allocation Type），精确匹配带有 `@EventListener` 或 `@TransactionalEventListener` 的接收端方法，并在调用图中创建“事件传递”虚拟边，实现无缝链路追踪。

#### 4.3 泛型集合数据流
Java 中广泛使用泛型集合（如 `List<T>`, `Map<K, V>`）传递数据。Astro-Probe 的 Java 语言前端在解析过程中，会将集合的操作虚拟化为字段读写：
- `list.add(element)` 转换为 `list.[element] = element` 的赋值操作。
- `list.get(index)` 转换为 `return = list.[element]` 的读取操作。
这使得全局 Points-To 指针分析能够精确地在跨方法集合传递中流动，极大地提高了 DFG 跟踪的完备性与最终审计的精确度。
