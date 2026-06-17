const assert = require('assert');
const { parseSymbolFQN } = require('./src/services/symbolResolver');

// 1. Copy getSimpleName from frontend to test it in Node
const getSimpleName = (fqn) => {
  let openParenIndex = fqn.indexOf('(');
  let beforeParams = '';
  let paramsContent = '';
  let afterParams = '';
  
  if (openParenIndex !== -1) {
    beforeParams = fqn.substring(0, openParenIndex);
    const closeParenIndex = fqn.lastIndexOf(')');
    if (closeParenIndex !== -1 && closeParenIndex > openParenIndex) {
      paramsContent = fqn.substring(openParenIndex + 1, closeParenIndex).trim();
      afterParams = fqn.substring(closeParenIndex + 1);
    } else {
      paramsContent = '';
      afterParams = fqn.substring(openParenIndex);
    }
  } else {
    beforeParams = fqn;
    paramsContent = '';
    afterParams = '';
  }

  let hashPart = '';
  const hashInAfterIndex = afterParams.indexOf('#');
  if (hashInAfterIndex !== -1) {
    hashPart = afterParams.substring(hashInAfterIndex);
  } else {
    const hashInBeforeIndex = beforeParams.indexOf('#');
    if (hashInBeforeIndex !== -1) {
      hashPart = beforeParams.substring(hashInBeforeIndex);
      beforeParams = beforeParams.substring(0, hashInBeforeIndex);
    }
  }

  // Parse paramsContent
  let paramsPart = '';
  if (openParenIndex !== -1) {
    if (paramsContent) {
      // Split parameters by comma at the top level
      const splitParams = (paramsStr) => {
        const result = [];
        let current = '';
        let depth = 0;
        for (let i = 0; i < paramsStr.length; i++) {
          const char = paramsStr[i];
          if (char === '<') {
            depth++;
            current += char;
          } else if (char === '>') {
            depth = Math.max(0, depth - 1);
            current += char;
          } else if (char === ',' && depth === 0) {
            result.push(current.trim());
            current = '';
          } else {
            current += char;
          }
        }
        if (current.trim()) {
          result.push(current.trim());
        }
        return result;
      };

      const parsedParams = splitParams(paramsContent);
      const simplifiedParams = parsedParams
        .map(param => param.replace(/[a-zA-Z0-9_$]+\./g, ''))
        .join(', ');
      paramsPart = `(${simplifiedParams})`;
    } else {
      paramsPart = '()';
    }
  }

  // Parse beforeParams
  // Split the class/method path by dot, ignoring dots inside <...> brackets
  const splitBeforeParams = (beforeParamsStr) => {
    const result = [];
    let current = '';
    let depth = 0;
    for (let i = 0; i < beforeParamsStr.length; i++) {
      const char = beforeParamsStr[i];
      if (char === '<') {
        depth++;
        current += char;
      } else if (char === '>') {
        depth = Math.max(0, depth - 1);
        current += char;
      } else if (char === '.' && depth === 0) {
        result.push(current.trim());
        current = '';
      } else {
        current += char;
      }
    }
    if (current.trim()) {
      result.push(current.trim());
    }
    return result;
  };

  const segments = splitBeforeParams(beforeParams);
  const lastSegments = segments.length > 2 ? segments.slice(-2) : segments;
  const processedSegments = lastSegments.map(segment => segment.replace(/[a-zA-Z0-9_$]+\./g, ''));
  const label = processedSegments.join('.');

  return `${label}${paramsPart}${hashPart}`;
};

// 2. Simulated elements generator from frontend GraphVisualizer
function buildElements(graphData) {
  const elements = [];
  const nodeIds = new Set();
  const rawEdges = graphData.edges || [];
  const rawNodes = graphData.nodes || [];

  const inferType = (fqn) => {
    if (
      fqn.startsWith('GET ') ||
      fqn.startsWith('POST ') ||
      fqn.startsWith('PUT ') ||
      fqn.startsWith('DELETE ') ||
      fqn.startsWith('PATCH ')
    ) {
      return 'route';
    }
    const lower = fqn.toLowerCase();
    if (lower.includes('controller')) return 'controller';
    if (lower.includes('service')) return 'service';
    if (lower.includes('repository') || lower.includes('repo')) return 'repository';
    return 'default';
  };

  // Add nodes explicitly
  rawNodes.forEach(node => {
    if (!nodeIds.has(node)) {
      nodeIds.add(node);
      elements.push({
        data: { id: node, label: getSimpleName(node), fqn: node, type: inferType(node) }
      });
    }
  });

  // Add nodes/edges from edges
  rawEdges.forEach((edge, index) => {
    const source = edge.caller || edge.from || edge.source;
    const target = edge.callee || edge.to || edge.target;

    if (!source || !target) return;

    if (!nodeIds.has(source)) {
      nodeIds.add(source);
      elements.push({
        data: { id: source, label: getSimpleName(source), fqn: source, type: inferType(source) }
      });
    }
    if (!nodeIds.has(target)) {
      nodeIds.add(target);
      elements.push({
        data: { id: target, label: getSimpleName(target), fqn: target, type: inferType(target) }
      });
    }

    elements.push({
      data: {
        id: `${source}->${target}#${edge.type || edge.edge_type || ''}-${index}`,
        source,
        target,
        isVirtual: edge.is_virtual || false,
        edgeType: edge.type || edge.edge_type || ''
      },
      classes: edge.type || edge.edge_type || ''
    });
  });

  return elements;
}

// 3. Simulated React hook execution for ResizeObserver environment check
function simulateResizeObserverHook(containerRef, cyRef) {
  // Same logic as useEffect
  if (!containerRef.current || !cyRef.current) return 'no-op (refs empty)';
  if (typeof ResizeObserver === 'undefined') {
    return 'safe return (ResizeObserver undefined)';
  }
  const observer = new ResizeObserver(() => {
    if (cyRef.current) {
      cyRef.current.resize();
    }
  });
  observer.observe(containerRef.current);
  return {
    observer,
    disconnect: () => observer.disconnect()
  };
}

// RUN TESTS
function runVerification() {
  console.log('=== STARTING EMPIRICAL CHALLENGER VERIFICATION ===\\n');
  let failures = 0;

  // --- TASK 1: getSimpleName Parsing ---
  console.log('--- TASK 1.1: Testing frontend getSimpleName ---');
  const frontendCases = [
    {
      input: 'com.example.simple.controller.UserController.getUserById(java.lang.Long)#id',
      expected: 'UserController.getUserById(Long)#id'
    },
    {
      input: 'com.example.Service.process(int, java.util.List<java.lang.String>)',
      expected: 'Service.process(int, List<String>)'
    },
    {
      input: 'com.example.Outer$Inner.method(Map<String,List<Integer>>)#var',
      expected: 'Outer$Inner.method(Map<String,List<Integer>>)#var'
    },
    {
      input: 'com.example.simple.controller.UserController',
      expected: 'controller.UserController'
    },
    {
      input: 'com.example.Class.method()',
      expected: 'Class.method()'
    },
    {
      input: 'Class.method()',
      expected: 'Class.method()'
    },
    {
      input: 'method()',
      expected: 'method()'
    },
    {
      input: 'com.example.Class.method(int[], String[][])#var',
      expected: 'Class.method(int[], String[][])#var'
    }
  ];

  frontendCases.forEach((tc, i) => {
    const actual = getSimpleName(tc.input);
    try {
      assert.strictEqual(actual, tc.expected);
      console.log(`[PASS] Case ${i + 1}: "${tc.input}" -> "${actual}"`);
    } catch (e) {
      console.error(`[FAIL] Case ${i + 1}: "${tc.input}"\\n  Expected: "${tc.expected}"\\n  Got:      "${actual}"`);
      failures++;
    }
  });

  // --- TASK 1.2: parseSymbolFQN Parsing ---
  console.log('\\n--- TASK 1.2: Testing backend parseSymbolFQN ---');
  const backendCases = [
    {
      input: 'com.example.simple.controller.UserController.getUserById(java.lang.Long)#id',
      expected: {
        classFqn: 'com.example.simple.controller.UserController',
        methodName: 'getUserById',
        methodParams: ['java.lang.Long'],
        variableName: 'id'
      }
    },
    {
      input: 'com.example.Service.process(int, java.util.List<java.lang.String>)',
      expected: {
        classFqn: 'com.example.Service',
        methodName: 'process',
        methodParams: ['int', 'java.util.List<java.lang.String>'],
        variableName: ''
      }
    },
    {
      input: 'com.example.Class#field',
      expected: {
        classFqn: 'com.example.Class',
        methodName: '',
        methodParams: null,
        variableName: 'field'
      }
    },
    {
      input: 'com.example.Class.<init>()',
      expected: {
        classFqn: 'com.example.Class',
        methodName: '<init>',
        methodParams: [],
        variableName: ''
      }
    },
    {
      input: 'Class.method(Map<String,List<Integer>>)#var',
      expected: {
        classFqn: 'Class',
        methodName: 'method',
        methodParams: ['Map<String,List<Integer>>'],
        variableName: 'var'
      }
    }
  ];

  backendCases.forEach((tc, i) => {
    const actual = parseSymbolFQN(tc.input);
    try {
      assert.deepStrictEqual(actual, tc.expected);
      console.log(`[PASS] Case ${i + 1}: "${tc.input}" -> ${JSON.stringify(actual)}`);
    } catch (e) {
      console.error(`[FAIL] Case ${i + 1}: "${tc.input}"\\n  Expected: ${JSON.stringify(tc.expected)}\\n  Got:      ${JSON.stringify(actual)}`);
      failures++;
    }
  });

  // --- TASK 2: Cytoscape Edge IDs Uniqueness ---
  console.log('\\n--- TASK 2: Testing Cytoscape Edge ID Uniqueness ---');
  const mockGraph = {
    nodes: [
      'com.example.Controller.handle',
      'com.example.Service.doWork',
      'com.example.Repo.save'
    ],
    edges: [
      // Multiple edges between identical nodes
      { caller: 'com.example.Controller.handle', callee: 'com.example.Service.doWork', type: 'CALL' },
      { caller: 'com.example.Controller.handle', callee: 'com.example.Service.doWork', type: 'CALL' },
      { caller: 'com.example.Controller.handle', callee: 'com.example.Service.doWork', type: 'READ' },
      // Same nodes, other direction
      { caller: 'com.example.Service.doWork', callee: 'com.example.Controller.handle', type: 'CALLBACK' },
      // Self loops
      { caller: 'com.example.Service.doWork', callee: 'com.example.Service.doWork', type: 'CALL' },
      { caller: 'com.example.Service.doWork', callee: 'com.example.Service.doWork', type: 'CALL' }
    ]
  };

  const elements = buildElements(mockGraph);
  const ids = elements.map(el => el.data.id);
  const uniqueIds = new Set(ids);

  try {
    assert.strictEqual(ids.length, uniqueIds.size, 'Duplicate IDs detected!');
    console.log(`[PASS] Generated ${ids.length} elements. All IDs are unique:`);
    ids.forEach(id => console.log(`  - ${id}`));
  } catch (e) {
    console.error(`[FAIL] Duplicate IDs detected! Total: ${ids.length}, Unique: ${uniqueIds.size}`);
    const duplicates = ids.filter((item, index) => ids.indexOf(item) !== index);
    console.error(`  Duplicates: ${JSON.stringify(duplicates)}`);
    failures++;
  }

  // --- TASK 3: ResizeObserver undefined check ---
  console.log('\\n--- TASK 3: Testing ResizeObserver Environment Check ---');
  
  // Test scenario A: ResizeObserver is undefined (like standard Node.js server/test environment)
  const backupResizeObserver = global.ResizeObserver;
  delete global.ResizeObserver;

  const mockContainerRef = { current: {} };
  const mockCyRef = { current: { resize: () => {} } };

  const resA = simulateResizeObserverHook(mockContainerRef, mockCyRef);
  console.log(`  Scenario A (ResizeObserver undefined): ${resA}`);
  try {
    assert.strictEqual(resA, 'safe return (ResizeObserver undefined)');
    console.log('[PASS] ResizeObserver === undefined behaves safely without crashes or throw.');
  } catch (e) {
    console.error('[FAIL] Expected early return from ResizeObserver check, got:', resA);
    failures++;
  }

  // Test scenario B: ResizeObserver is defined (browser-like environment)
  class MockResizeObserver {
    constructor(callback) {
      this.callback = callback;
    }
    observe(el) {
      this.observed = el;
    }
    disconnect() {
      this.disconnected = true;
    }
  }
  global.ResizeObserver = MockResizeObserver;

  const resB = simulateResizeObserverHook(mockContainerRef, mockCyRef);
  console.log(`  Scenario B (ResizeObserver defined): ${resB ? 'initialized' : 'failed'}`);
  try {
    assert.ok(resB.observer instanceof MockResizeObserver);
    assert.strictEqual(resB.observer.observed, mockContainerRef.current);
    resB.disconnect();
    assert.strictEqual(resB.observer.disconnected, true);
    console.log('[PASS] ResizeObserver defined behaves correctly, observers element, and disconnects.');
  } catch (e) {
    console.error('[FAIL] ResizeObserver flow mismatch:', e.message);
    failures++;
  }

  // Restore global
  if (backupResizeObserver) {
    global.ResizeObserver = backupResizeObserver;
  } else {
    delete global.ResizeObserver;
  }

  console.log('\\n=== VERIFICATION SUMMARY ===');
  if (failures === 0) {
    console.log('ALL VERIFICATION TASKS PASSED! Verdict: PASS');
    process.exit(0);
  } else {
    console.error(`${failures} VERIFICATION TASK(S) FAILED. Verdict: FAIL`);
    process.exit(1);
  }
}

runVerification();
