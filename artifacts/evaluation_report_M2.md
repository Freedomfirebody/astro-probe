# Milestone 2 Project Evaluation Report

**Date**: 2026-06-11
**Evaluator**: Project Evaluator M2 (Teamwork Agent: Reviewer / Critic)
**Project Workspace**: `c:\Development\Project\Rust\astro-probe`
**Milestone**: M2 (Incremental Analysis)

---

## 1. Conclusion

**Status**: **PASS**

* **Requirements Coverage Score**: **100%** (all Milestone 2 criteria fully met)
* **Architectural Deviation Rating**: **Low** (all design deviations are acceptable layout adjustments, with zero Action-Required or Blocking deviations)

---

## 2. Requirements Coverage (100%)

Milestone 2 requires the implementation of an incremental program analysis engine for Java bytecode and source projects. The evaluation of the implementation against the original requirements in `ORIGINAL_REQUEST.md` is detailed below.

### 2.1. Incremental Analysis Speedup (medium-spring)
* **Requirement**: Modifying a single file in the `medium-spring` test sample and running a re-analysis must take less than 50% of the full initial build/analysis time (Speedup <50%).
* **Implementation**: Implemented in `crates/astro-probe-java/src/parser.rs` via a `file_hashes` SQLite table that tracks SHA-256 hashes of all source files. On re-analysis, the system computes hashes for current files, detects modified/deleted files, purges old facts for classes in dirty files, and parses only dirty/new files.
* **Verification**: Verified via `test_perf_benchmark_medium_spring` in `crates/astro-probe-tests/tests/perf_benchmark.rs`. The release test shows:
  * Full initial analysis of medium-spring: ~182ms
  * Incremental analysis of medium-spring: ~54ms
  * Speedup ratio: **29.82%** (well below the <50% requirement threshold).

### 2.2. Supernode Bypassing (Object.toString())
* **Requirement**: Large, high-degree nodes (supernodes) such as `java.lang.Object.toString()` must be bypassed to avoid scaling bottlenecks and infinite call graph recursion.
* **Implementation**: Implemented in `crates/astro-probe-core/src/cg.rs` (lines 313-331). The PointsToSolver dynamically identifies callee FQNs with an in-degree > 500 and statically matches built-in high-traffic methods, including `java.lang.Object.toString()`, `java.lang.StringBuilder.toString()`, and `java.lang.StringBuffer.toString()`. Instead of analyzing their bodies, the solver returns a synthetic `SupernodeReturn:callee_fqn` allocation.
* **Verification**: Verified via `test_supernode_detection_and_bypass` in `crates/astro-probe-tests/tests/integration.rs` and `test_validation_to_string_call_chains_bounded` in `crates/astro-probe-tests/tests/validation_tests.rs`. Both confirm that calls to `Object.toString()` return bounded synthetic allocations and do not propagate call graph edges downstream.

### 2.3. Third-Party Method Summaries Bytecode Extraction
* **Requirement**: Extract data-flow summaries directly from third-party library bytecode (JAR dependencies) to handle library calls without full transit analysis.
* **Implementation**: Implemented in `crates/astro-probe-java/src/jar.rs`. The JAR analyzer utilizes the `cafebabe` crate to parse class files and inspect Java bytecode instructions. If a method parameter is directly returned via a return instruction (areturn, ireturn, etc., opcodes `172..=176`), it inserts a method summary entry in the `cached_method_summaries` table. The solver reads these summaries and directly propagates points-to sets from argument variables to the return variable without analyzing the library method body.
* **Verification**: Verified via `test_copy_jar_facts_to_local_method_summaries` in `crates/astro-probe-tests/tests/integration.rs` and `test_validation_method_summaries_from_bytecode` in `crates/astro-probe-tests/tests/validation_tests.rs`. A synthetic JAR file is parsed, extracting the summary `Identity.f(java.lang.Object) -> param 0`, and points-to propagation is successfully verified across it.

### 2.4. Concrete Target Call Chains (OrderService -> UserService -> ProductService)
* **Requirement**: Spring dependency injection components must resolve interface call sites to concrete implementations, e.g., resolving the chain from `OrderService` to `UserService` and `ProductService`.
* **Implementation**: Implemented in `crates/astro-probe-java/src/di.rs` (DependencyInjectionAnalyzer) and `crates/astro-probe-core/src/cg.rs` (PointsToSolver). The DI analyzer scans for Spring components (`@Service`, `@Component`, `@Repository`, `@Autowired`) and generates assignments mapping class implementations to their target interface autowire fields. The solver propagates concrete types to virtual call sites and performs dynamic dispatch lookup over the class hierarchy.
* **Verification**: Verified via `test_validation_medium_spring_call_chains` in `crates/astro-probe-tests/tests/validation_tests.rs`. The test executes a full analysis on `medium-spring` and asserts that call edges from `OrderServiceImpl.createOrder` are concrete edges to `UserServiceImpl.findById`, `ProductServiceImpl.findById`, and `ProductServiceImpl.updateStock` rather than their abstract interfaces.

### 2.5. Nacos Stress Re-analysis (< 30s)
* **Requirement**: Incremental re-analysis on large codebases (tested with Nacos, which contains over 2,200 source files) must execute in under 30 seconds.
* **Implementation**: The combination of file-level hashing, restricted dirty-fact purging, in-memory points-to fixed-point maps, SQLite WAL journaling, and bulk transaction commits optimizes re-analysis.
* **Verification**: Verified via `test_perf_benchmark_nacos` in `crates/astro-probe-tests/tests/perf_benchmark.rs`. In release mode, the incremental analysis of Nacos completes in **10.11 seconds**, satisfying the `<30s` requirement.

### 2.6. Test Suite Completeness
* **Requirement**: All workspace tests pass, including at least 5 new tests.
* **Implementation**: Over 36 tests pass across the codebase (including unit, integration, validation, and performance benchmark suites).
* **Verification**: Running `cargo test --workspace` completed successfully with zero failures. Ten new tests validating M2 requirements (transitive DFG reduction, supernode detection, bytecode summaries, incremental analysis speed, and DI call chains) are integrated into `integration.rs` and `validation_tests.rs`.

---

## 3. Architectural Deviation (Low)

The implementation aligns with the architectural design defined in `PROJECT.md`, with only a few minor layout adjustments.

### 3.1. Detailed Deviation List
* **Acceptable Deviations**:
  1. **Consolidation of Core Engine Submodules**: Points-to solver and call graph builder are directly in `cg.rs` and the DFG lineage builder is in `dfg.rs` inside `astro-probe-core`, instead of under a nested `engine/` directory (e.g. `crates/astro-probe-core/src/engine/points_to.rs`). This structure simplifies crate module exports and avoids excessive nesting of cargo modules.
  2. **Consolidation of Database Submodules**: Database connection pooling, initialization, and table definitions are unified in `crates/astro-probe-db/src/lib.rs` instead of split into `schema.rs`, `pool.rs`, and `migration.rs`. This reduces boilerplate and keeps DB schema management concise.
* **Action-Required Deviations**:
  * None.
* **Blocking Deviations**:
  * None.

### 3.2. Trait Design Compliance
All 5 core traits (`SourceParser`, `TypeSystem`, `DependencyAnalyzer`, `FrameworkAnalyzer`, `LanguageFrontend`) defined in `crates/astro-probe-core/src/traits.rs` are compliant, utilizing associated `Error` types rather than generic type parameters.

---

## 4. Cumulative Technical Debt

The following technical debt items have been identified:

1. **Direct Path Interpolation in `ATTACH DATABASE` (Low Risk)**:
   In `jar.rs` (line 1132), the global cache DB is attached using string formatting of the database path, because SQLite does not support path parameterization for `ATTACH DATABASE`. The string is escaped safely by replacing single quotes with double single quotes and backslashes with slashes.
2. **Direct stdout Logging (Medium Risk)**:
   The analysis engine and parsers write log telemetry directly to stdout via `println!` macros. These should be refactored to use the standard `tracing` library.
3. **Timing Benchmark Sensitivity under Debug Profile (Low Risk)**:
   Timings are asserted inside integration tests. Under unoptimized debug environments, constant setup costs (workspace creation, file IO, database connection) can lead to flaky test failures depending on host CPU scheduling. Performance tests should be run in `--release`.
4. **Call Edge Index Management (Low Risk)**:
   Unlike the DFG builder, the call graph solver (`cg.rs`) does not drop/disable the caller/callee database indexes (`idx_call_edges_caller` / `idx_call_edges_callee`) before bulk inserts, which can slow down inserts slightly for large codebases.

---

## 5. Subsequent Milestone Impact Analysis

* **Milestone 3 (Routes & Events)**:
  No architectural blockers exist. The Spring MVC routes can easily map to endpoints using the `FrameworkAnalyzer` trait. Event lineage can be trace-propagated using dispatch rules in `PointsToSolver`.
* **Milestone 4 (Build Tools & k-CFA)**:
  Adding Maven/Gradle dependency resolution fits perfectly inside the `DependencyAnalyzer` trait. Implementing k-CFA (k=1) context-sensitive call graphs will require extending points-to set variables to include caller context, which is easily doable without breaking `core` abstractions.
* **Milestone 5 (Persistence & SSE)**:
  Persisting workspace states across restarts is directly supported by the current database schema design. Migrating `println!` messages to structured tracing will be needed to satisfy the observability requirement.
