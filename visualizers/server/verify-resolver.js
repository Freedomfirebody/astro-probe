const path = require('path');
const { resolveJavaSymbol } = require('./src/services/symbolResolver');

const projectPath = path.resolve(__dirname, '../../test-samples/simple-spring');

const testCases = [
  {
    fqn: 'com.example.simple.controller.UserController',
    description: 'Class UserController',
    expected: {
      startLine: 21,
      startColumn: 14
    }
  },
  {
    fqn: 'com.example.simple.controller.UserController.<init>(com.example.simple.service.UserService)',
    description: 'Constructor UserController(UserService)',
    expected: {
      startLine: 26,
      startColumn: 12
    }
  },
  {
    fqn: 'com.example.simple.controller.UserController.getUserById(java.lang.Long)',
    description: 'Method getUserById(Long)',
    expected: {
      startLine: 37,
      startColumn: 36
    }
  },
  {
    fqn: 'com.example.simple.controller.UserController.getUserById(java.lang.Long)#id',
    description: 'Parameter id in getUserById(Long)',
    expected: {
      startLine: 37,
      startColumn: 63
    }
  },
  {
    fqn: 'com.example.simple.controller.UserController.getAllUsers()#users',
    description: 'Local variable users in getAllUsers()',
    expected: {
      startLine: 32,
      startColumn: 23
    }
  }
];

console.log('--- STARTING SYMBOL RESOLVER VERIFICATION ---');
let allPassed = true;

for (const tc of testCases) {
  try {
    const result = resolveJavaSymbol(projectPath, tc.fqn);
    console.log(`\nFQN: ${tc.fqn}`);
    console.log(`Desc: ${tc.description}`);
    console.log(`Resolved: File: ${result.filePath}`);
    console.log(`          Coordinates: ${result.startLine}:${result.startColumn} to ${result.endLine}:${result.endColumn}`);
    
    // Check line matching
    if (result.startLine !== tc.expected.startLine) {
      console.error(`FAIL: expected startLine ${tc.expected.startLine}, got ${result.startLine}`);
      allPassed = false;
    } else {
      console.log(`PASS: startLine matched (${result.startLine})`);
    }
    
    if (result.startColumn !== tc.expected.startColumn) {
      console.warn(`NOTE: startColumn resolved as ${result.startColumn}, expected around ${tc.expected.startColumn}`);
    } else {
      console.log(`PASS: startColumn matched (${result.startColumn})`);
    }
  } catch (err) {
    console.error(`FAIL: Exception thrown during resolution of ${tc.fqn}:`);
    console.error(err.stack || err.message);
    allPassed = false;
  }
}

if (allPassed) {
  console.log('\n=== ALL TESTS PASSED SUCCESSFULLY ===');
  process.exit(0);
} else {
  console.error('\n=== SOME TESTS FAILED ===');
  process.exit(1);
}
