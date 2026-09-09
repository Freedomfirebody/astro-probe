const path = require('path');
const { resolveJavaSymbol } = require('./src/services/symbolResolver');

const projectPath = path.resolve(__dirname, '../../test-samples/simple-spring');

const testCases = [
  // 1. Generics
  {
    fqn: 'com.example.simple.util.TestFeatures.processList(java.util.List)',
    description: 'Generic argument: match by simplified class name',
    expected: {
      startLine: 15,
      startColumn: 17
    }
  },
  {
    fqn: 'com.example.simple.util.TestFeatures.processList(java.util.List<java.lang.String>)',
    description: 'Generic argument: match with full generic FQN signature',
    expected: {
      startLine: 15,
      startColumn: 17
    }
  },
  {
    fqn: 'com.example.simple.util.TestFeatures.genericMethod(T)',
    description: 'Generic method: match with generic parameter type name T',
    expected: {
      startLine: 11,
      startColumn: 18
    }
  },
  {
    fqn: 'com.example.simple.util.TestFeatures.genericMethod(java.lang.Object)',
    description: 'Generic method: match with JVM erased type Object (expected fail/pass comparison)',
    expected: {
      startLine: 11,
      startColumn: 18
    }
  },

  // 2. Overloaded methods
  {
    fqn: 'com.example.simple.util.TestFeatures.overload()',
    description: 'Overloaded method: no args',
    expected: {
      startLine: 19,
      startColumn: 17
    }
  },
  {
    fqn: 'com.example.simple.util.TestFeatures.overload(java.lang.String)',
    description: 'Overloaded method: single String arg',
    expected: {
      startLine: 22,
      startColumn: 17
    }
  },
  {
    fqn: 'com.example.simple.util.TestFeatures.overload(int)',
    description: 'Overloaded method: single primitive int arg',
    expected: {
      startLine: 25,
      startColumn: 17
    }
  },
  {
    fqn: 'com.example.simple.util.TestFeatures.overload(java.lang.String,int)',
    description: 'Overloaded method: multiple arguments (String, int)',
    expected: {
      startLine: 28,
      startColumn: 17
    }
  },

  // 3. Parameters and local variables (verify lines and columns are exactly correct)
  {
    fqn: 'com.example.simple.util.TestFeatures.processList(java.util.List)#list',
    description: 'Parameter: list in processList(List)',
    expected: {
      startLine: 15,
      startColumn: 42
    }
  },
  {
    fqn: 'com.example.simple.util.TestFeatures.processList(java.util.List)#first',
    description: 'Local variable: first in processList(List)',
    expected: {
      startLine: 16,
      startColumn: 16
    }
  },

  // 4. Static initializers and Implicit constructors
  {
    fqn: 'com.example.simple.util.TestFeatures.<clinit>()',
    description: 'Static initializer <clinit>',
    expected: {
      startLine: 7,
      startColumn: 5
    }
  },
  {
    fqn: 'com.example.simple.util.ImplicitClass.<init>()',
    description: 'Implicit constructor on class without explicit constructor declaration',
    expected: {
      // Since it's implicit, it is expected to fail or behave in a specific way
      startLine: 3,
      startColumn: 14
    }
  }
];

console.log('--- STARTING EXTENDED SYMBOL RESOLVER VERIFICATION ---');
let allPassed = true;
const results = [];

for (const tc of testCases) {
  const resultObj = {
    fqn: tc.fqn,
    description: tc.description,
    status: 'UNKNOWN',
    details: ''
  };

  try {
    const result = resolveJavaSymbol(projectPath, tc.fqn);
    resultObj.resolved = {
      filePath: result.filePath,
      startLine: result.startLine,
      startColumn: result.startColumn,
      endLine: result.endLine,
      endColumn: result.endColumn
    };

    let matched = true;
    let detailMsg = [];
    if (result.startLine !== tc.expected.startLine) {
      detailMsg.push(`startLine mismatch (got ${result.startLine}, expected ${tc.expected.startLine})`);
      matched = false;
    }
    if (result.startColumn !== tc.expected.startColumn) {
      detailMsg.push(`startColumn mismatch (got ${result.startColumn}, expected ${tc.expected.startColumn})`);
      matched = false;
    }

    if (matched) {
      resultObj.status = 'PASS';
      resultObj.details = `Matched perfectly at ${result.startLine}:${result.startColumn}`;
    } else {
      resultObj.status = 'FAIL';
      resultObj.details = detailMsg.join(', ');
      allPassed = false;
    }
  } catch (err) {
    resultObj.status = 'FAIL';
    resultObj.details = `Error: ${err.message}`;
    allPassed = false;
  }
  results.push(resultObj);
}

// Print results table
console.log('\n--- VERIFICATION RESULTS ---');
results.forEach((r, idx) => {
  console.log(`[${idx + 1}] ${r.status} - ${r.fqn}`);
  console.log(`    Desc: ${r.description}`);
  console.log(`    Details: ${r.details}`);
  if (r.resolved) {
    console.log(`    Resolved location: ${r.resolved.startLine}:${r.resolved.startColumn} -> ${r.resolved.endLine}:${r.resolved.endColumn}`);
  }
  console.log('');
});

if (allPassed) {
  console.log('=== ALL TESTS PASSED ===');
  process.exit(0);
} else {
  console.log('=== SOME TESTS FAILED / UNMATCHED ===');
  process.exit(1);
}
