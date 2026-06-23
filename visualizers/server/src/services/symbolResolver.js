const fs = require('fs');
const path = require('path');
const axios = require('axios');
const javaParser = require('java-parser');

const RUST_BACKEND_URL = process.env.RUST_BACKEND_URL || 'http://127.0.0.1:8080';

/**
 * Parses a Java symbol FQN into its constituent components.
 * @param {string} fqn - The symbol FQN to parse.
 * @returns {Object} Parsed parts: { classFqn, methodName, methodParams, variableName }
 */
function parseSymbolFQN(fqn) {
  let remaining = fqn;
  let variableName = '';

  // 1. Extract variable or parameter name (after '#')
  const hashIdx = remaining.indexOf('#');
  if (hashIdx !== -1) {
    variableName = remaining.substring(hashIdx + 1);
    remaining = remaining.substring(0, hashIdx);
  }

  // 2. Extract method signature (between '(' and ')')
  let methodName = '';
  let methodParams = null; // null means class level

  const openParenIdx = remaining.indexOf('(');
  if (openParenIdx !== -1) {
    const closeParenIdx = remaining.lastIndexOf(')');
    if (closeParenIdx !== -1) {
      const paramsStr = remaining.substring(openParenIdx + 1, closeParenIdx).trim();
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
      methodParams = paramsStr ? splitParams(paramsStr) : [];
      
      const methodWithClass = remaining.substring(0, openParenIdx);
      const lastDot = methodWithClass.lastIndexOf('.');
      if (lastDot !== -1) {
        methodName = methodWithClass.substring(lastDot + 1);
        remaining = methodWithClass.substring(0, lastDot);
      } else {
        methodName = methodWithClass;
        remaining = '';
      }
    }
  }

  const classFqn = remaining;

  return {
    classFqn,
    methodName,
    methodParams,
    variableName
  };
}

const PRIMITIVE_TO_BOXED = {
  'int': 'Integer',
  'long': 'Long',
  'double': 'Double',
  'float': 'Float',
  'boolean': 'Boolean',
  'char': 'Character',
  'short': 'Short',
  'byte': 'Byte',
  'void': 'Void'
};

function normalizeTypeName(typeStr) {
  if (!typeStr) return '';
  let type = typeStr.trim();
  const isArray = type.endsWith('[]');
  if (isArray) {
    type = type.substring(0, type.length - 2).trim();
  }
  
  // Extract the last identifier component (handling both dots and dollar signs)
  const parts = type.split(/[.$]/);
  let base = parts[parts.length - 1] || '';
  
  // Map primitive to boxed type
  base = PRIMITIVE_TO_BOXED[base] || base;
  
  return isArray ? base + '[]' : base;
}

/**
 * Helper to convert fully qualified type to its simple type.
 * e.g. "java.lang.Long" -> "Long", "int[]" -> "int[]", "List<UserDto>" -> "List"
 */
function getSimpleTypeName(fqTypeName) {
  if (!fqTypeName) return '';
  let type = fqTypeName;
  const isArray = type.endsWith('[]');
  if (isArray) {
    type = type.substring(0, type.length - 2);
  }
  const ltIdx = type.indexOf('<');
  if (ltIdx !== -1) {
    type = type.substring(0, ltIdx);
  }
  const lastDot = type.lastIndexOf('.');
  if (lastDot !== -1) {
    type = type.substring(lastDot + 1);
  }
  return isArray ? type + '[]' : type;
}

/**
 * Helper to get the type name string from an AST unannType node.
 * Strip generic type parameters and handle array/varargs.
 */
function getTypeNameString(unannTypeNode) {
  if (!unannTypeNode) return '';
  let parts = [];
  let isArray = false;
  let isVarargs = false;

  function walk(node) {
    if (!node) return;
    if (Array.isArray(node)) {
      node.forEach(walk);
      return;
    }
    if (typeof node === 'object') {
      if (node.image && node.startLine !== undefined) {
        const img = node.image;
        if (img === '[') {
          isArray = true;
        } else if (img === '...') {
          isVarargs = true;
        } else if (img !== ']' && img !== '<' && img !== '>') {
          parts.push(img);
        }
        return;
      }
      const name = node.name;
      if (name === 'typeArguments' || name === 'typeParameters') {
        return;
      }
      if (node.children) {
        for (const key in node.children) {
          walk(node.children[key]);
        }
      }
    }
  }

  walk(unannTypeNode);

  const identifiers = parts.filter(p => /^[a-zA-Z_$][a-zA-Z0-9_$]*$/.test(p));
  if (identifiers.length === 0) return '';

  let baseType = identifiers[identifiers.length - 1];
  if (isArray || isVarargs) {
    baseType += '[]';
  }
  return baseType;
}

/**
 * Compare method parameters from FQN and AST.
 */
function matchMethodParams(declaratorNode, methodParams) {
  if (methodParams === null) return true;
  
  const formalParamsNode = declaratorNode.children?.formalParameterList?.[0];
  const astParams = formalParamsNode?.children?.formalParameter || [];

  if (astParams.length !== methodParams.length) return false;

  for (let i = 0; i < astParams.length; i++) {
    const astParam = astParams[i];
    const astTypeName = getTypeNameString(astParam.children?.unannType?.[0]);
    const fqnTypeName = getSimpleTypeName(methodParams[i]);
    
    if (normalizeTypeName(astTypeName) !== normalizeTypeName(fqnTypeName)) {
      return false;
    }
  }

  return true;
}

/**
 * Returns location range of a CST node.
 */
function getNodeRange(node) {
  if (!node) return null;
  if (node.startLine !== undefined) {
    return {
      startLine: node.startLine,
      startColumn: node.startColumn,
      endLine: node.endLine,
      endColumn: node.endColumn
    };
  }
  if (node.location) {
    return {
      startLine: node.location.startLine,
      startColumn: node.location.startColumn,
      endLine: node.location.endLine,
      endColumn: node.location.endColumn
    };
  }

  let minLine = Infinity, minCol = Infinity;
  let maxLine = -Infinity, maxCol = -Infinity;

  function traverse(child) {
    if (!child) return;
    if (Array.isArray(child)) {
      child.forEach(traverse);
    } else if (typeof child === 'object') {
      if (child.startLine !== undefined) {
        if (child.startLine < minLine || (child.startLine === minLine && child.startColumn < minCol)) {
          minLine = child.startLine;
          minCol = child.startColumn;
        }
        if (child.endLine > maxLine || (child.endLine === maxLine && child.endColumn > maxCol)) {
          maxLine = child.endLine;
          maxCol = child.endColumn;
        }
      } else if (child.location && child.location.startLine !== undefined) {
        const loc = child.location;
        if (loc.startLine < minLine || (loc.startLine === minLine && loc.startColumn < minCol)) {
          minLine = loc.startLine;
          minCol = loc.startColumn;
        }
        if (loc.endLine > maxLine || (loc.endLine === maxLine && loc.endColumn > maxCol)) {
          maxLine = loc.endLine;
          maxCol = loc.endColumn;
        }
      } else {
        for (const key in child) {
          traverse(child[key]);
        }
      }
    }
  }

  traverse(node);
  return minLine === Infinity ? null : { startLine: minLine, startColumn: minCol, endLine: maxLine, endColumn: maxCol };
}

/**
 * Find the package/class identifier token of a class declaration node.
 */
function getDeclarationName(declarationNode) {
  const children = declarationNode.children;
  if (!children) return null;

  let typeIdNode = null;
  if (children.typeIdentifier?.[0]) {
    typeIdNode = children.typeIdentifier[0];
  } else if (children.classHeader?.[0]?.children?.typeIdentifier?.[0]) {
    typeIdNode = children.classHeader[0].children.typeIdentifier[0];
  } else if (children.interfaceHeader?.[0]?.children?.typeIdentifier?.[0]) {
    typeIdNode = children.interfaceHeader[0].children.typeIdentifier[0];
  }

  if (typeIdNode && typeIdNode.children?.Identifier?.[0]) {
    return typeIdNode.children.Identifier[0];
  }

  let foundToken = null;
  function findIdentifierToken(node) {
    if (foundToken) return;
    if (node.image && node.startLine !== undefined) {
      return;
    }
    if (node.children) {
      if (node.children.Identifier?.[0]) {
        foundToken = node.children.Identifier[0];
        return;
      }
      for (const key in node.children) {
        if (key === 'classBody' || key === 'interfaceBody' || key === 'enumBody' || key === 'recordBody') {
          continue;
        }
        node.children[key].forEach(findIdentifierToken);
      }
    }
  }
  findIdentifierToken(declarationNode);
  return foundToken;
}

/**
 * Returns classBody, interfaceBody, etc.
 */
function findClassBody(classNode) {
  if (!classNode || !classNode.children) return null;
  const children = classNode.children;
  if (children.classBody?.[0]) return children.classBody[0];
  if (children.interfaceBody?.[0]) return children.interfaceBody[0];
  if (children.enumBody?.[0]) return children.enumBody[0];
  if (children.recordBody?.[0]) return children.recordBody[0];
  if (children.annotationTypeBody?.[0]) return children.annotationTypeBody[0];

  let body = null;
  function search(node) {
    if (body) return;
    if (!node || typeof node !== 'object') return;
    if (Array.isArray(node)) {
      node.forEach(search);
      return;
    }
    if (node.name && node.name.endsWith('Body')) {
      body = node;
      return;
    }
    if (node.children) {
      for (const key in node.children) {
        search(node.children[key]);
      }
    }
  }
  search(classNode);
  return body;
}

/**
 * Search inner classes recursively.
 */
function findNestedClassNode(startNode, innerClassNames) {
  let currentNode = startNode;

  for (const name of innerClassNames) {
    let nextNode = null;
    const bodyNode = findClassBody(currentNode);
    if (!bodyNode) return null;

    function searchBody(node) {
      if (nextNode) return;
      if (!node || typeof node !== 'object') return;

      if (Array.isArray(node)) {
        node.forEach(searchBody);
        return;
      }

      const nodeName = node.name;
      if (
        nodeName === 'classDeclaration' ||
        nodeName === 'interfaceDeclaration' ||
        nodeName === 'enumDeclaration' ||
        nodeName === 'recordDeclaration' ||
        nodeName === 'annotationTypeDeclaration'
      ) {
        const idToken = getDeclarationName(node);
        if (idToken && idToken.image === name) {
          nextNode = node;
          return;
        }
      }

      if (node.children) {
        for (const key in node.children) {
          if (
            key === 'classDeclaration' ||
            key === 'interfaceDeclaration' ||
            key === 'enumDeclaration' ||
            key === 'recordDeclaration' ||
            key === 'annotationTypeDeclaration'
          ) {
            const childList = node.children[key];
            for (const child of childList) {
              const idToken = getDeclarationName(child);
              if (idToken && idToken.image === name) {
                nextNode = child;
                return;
              }
            }
            continue;
          }
          searchBody(node.children[key]);
        }
      }
    }

    searchBody(bodyNode);
    if (!nextNode) return null;
    currentNode = nextNode;
  }

  return currentNode;
}

/**
 * Find outer class declaration node.
 */
function findOuterClassNode(cst, outerClassName) {
  let foundNode = null;

  function search(node) {
    if (foundNode) return;
    if (!node || typeof node !== 'object') return;

    if (Array.isArray(node)) {
      node.forEach(search);
      return;
    }

    const nodeName = node.name;
    if (
      nodeName === 'classDeclaration' ||
      nodeName === 'interfaceDeclaration' ||
      nodeName === 'enumDeclaration' ||
      nodeName === 'recordDeclaration' ||
      nodeName === 'annotationTypeDeclaration'
    ) {
      const idToken = getDeclarationName(node);
      if (idToken && idToken.image === outerClassName) {
        foundNode = node;
        return;
      }
    }

    if (node.children) {
      for (const key in node.children) {
        // Do not search inside nested class declarations at this stage
        if (
          key === 'classDeclaration' ||
          key === 'interfaceDeclaration' ||
          key === 'enumDeclaration' ||
          key === 'recordDeclaration' ||
          key === 'annotationTypeDeclaration'
        ) {
          continue;
        }
        search(node.children[key]);
      }
    }
  }

  search(cst);
  return foundNode;
}

/**
 * Matches a candidate method declaration.
 */
function matchMethod(node, targetMethodName, targetMethodParams) {
  const name = node.name;
  if (name === 'methodDeclaration') {
    const header = node.children?.methodHeader?.[0];
    const declarator = header?.children?.methodDeclarator?.[0];
    const identifierNode = declarator?.children?.Identifier?.[0];
    const currentMethodName = identifierNode?.image;

    if (currentMethodName === targetMethodName) {
      return matchMethodParams(declarator, targetMethodParams) ? node : null;
    }
  } else if (name === 'constructorDeclaration' && targetMethodName === '<init>') {
    const declarator = node.children?.constructorDeclarator?.[0];
    return matchMethodParams(declarator, targetMethodParams) ? node : null;
  } else if (name === 'staticInitializer' && targetMethodName === '<clinit>') {
    return node;
  } else if (name === 'compactConstructorDeclaration' && targetMethodName === '<init>') {
    return node;
  }
  return null;
}

/**
 * Returns method name identifier token.
 */
function getMethodIdentifierNode(methodNode) {
  if (!methodNode) return null;
  const name = methodNode.name;
  const children = methodNode.children;
  if (!children) return null;

  if (name === 'methodDeclaration') {
    const header = children.methodHeader?.[0];
    const declarator = header?.children?.methodDeclarator?.[0];
    return declarator?.children?.Identifier?.[0];
  } else if (name === 'constructorDeclaration') {
    const declarator = children.constructorDeclarator?.[0];
    const simpleTypeName = declarator?.children?.simpleTypeName?.[0];
    return simpleTypeName?.children?.Identifier?.[0];
  } else if (name === 'staticInitializer') {
    return children.Static?.[0];
  } else if (name === 'compactConstructorDeclaration') {
    return children.simpleTypeName?.[0]?.children?.Identifier?.[0];
  }
  return null;
}

/**
 * Find variable or parameter inside a method/constructor body.
 */
function findVariableOrParameterNode(methodNode, targetVarName) {
  if (!methodNode) return null;

  const name = methodNode.name;

  // 1. Search formal parameters
  const declarator = name === 'methodDeclaration'
    ? methodNode.children?.methodHeader?.[0]?.children?.methodDeclarator?.[0]
    : methodNode.children?.constructorDeclarator?.[0];

  if (declarator) {
    const formalParams = declarator.children?.formalParameterList?.[0];
    const paramList = formalParams?.children?.formalParameter || [];
    for (const param of paramList) {
      const varDeclaratorId = param.children?.variableDeclaratorId?.[0];
      const paramIdNode = varDeclaratorId?.children?.Identifier?.[0];
      if (paramIdNode && paramIdNode.image === targetVarName) {
        return paramIdNode;
      }
    }
  }

  // 2. Search body recursively for local variables
  let bodyNode = null;
  if (name === 'methodDeclaration') {
    bodyNode = methodNode.children?.methodBody?.[0];
  } else if (name === 'constructorDeclaration') {
    bodyNode = methodNode.children?.constructorBody?.[0];
  } else if (name === 'staticInitializer' || name === 'compactConstructorDeclaration') {
    bodyNode = methodNode.children?.block?.[0];
  }

  let foundNode = null;
  if (bodyNode) {
    function walk(node) {
      if (foundNode) return;
      if (!node || typeof node !== 'object') return;

      if (Array.isArray(node)) {
        node.forEach(walk);
        return;
      }

      const nodeName = node.name;
      if (nodeName === 'variableDeclaratorId') {
        const idToken = node.children?.Identifier?.[0];
        if (idToken && idToken.image === targetVarName) {
          foundNode = idToken;
          return;
        }
      }

      if (nodeName === 'catchFormalParameter') {
        const varDeclaratorId = node.children?.variableDeclaratorId?.[0];
        const idToken = varDeclaratorId?.children?.Identifier?.[0];
        if (idToken && idToken.image === targetVarName) {
          foundNode = idToken;
          return;
        }
      }

      if (node.children) {
        for (const key in node.children) {
          walk(node.children[key]);
        }
      }
    }
    walk(bodyNode);
  }

  return foundNode;
}

/**
 * SQLite mapping helper try-catch.
 */
function getPathFromDb(projectPath, dbPath, outerClassFqn) {
  let db = null;
  try {
    if (dbPath && fs.existsSync(dbPath)) {
      const Database = require('better-sqlite3');
      db = new Database(dbPath, { readonly: true });
      const stmt = db.prepare('SELECT file_path FROM file_facts_metadata WHERE class_fqn = ?');
      const row = stmt.get(outerClassFqn);
      if (row && row.file_path) {
        if (path.isAbsolute(row.file_path)) {
          return row.file_path;
        } else {
          return path.resolve(projectPath, row.file_path);
        }
      }
    }
  } catch (err) {
    // ignore
  } finally {
    if (db) {
      try {
        db.close();
      } catch (e) {
        // ignore
      }
    }
  }
  return null;
}

/**
 * Searches the filesystem for Outer.java matching the package structure FQN.
 */
function findFileByFqn(dir, fqn) {
  const relSuffix = fqn.replace(/\./g, path.sep) + '.java';

  // Try direct convention under src/main/java
  const mainPath = path.join(dir, 'src', 'main', 'java', relSuffix);
  if (fs.existsSync(mainPath)) return mainPath;

  // Try direct convention under src/test/java
  const testPath = path.join(dir, 'src', 'test', 'java', relSuffix);
  if (fs.existsSync(testPath)) return testPath;

  // Fallback: search recursively
  const parts = fqn.split('.');
  const fileName = parts[parts.length - 1] + '.java';
  const suffix = parts.join(path.sep) + '.java';

  let foundPath = null;
  const visited = new Set();
  const MAX_DEPTH = 15;

  function walk(currentDir, depth = 0) {
    if (foundPath) return;
    if (depth > MAX_DEPTH) return;

    let realCurrentDir;
    try {
      realCurrentDir = fs.realpathSync(currentDir);
    } catch (err) {
      return;
    }

    if (visited.has(realCurrentDir)) return;
    visited.add(realCurrentDir);

    const base = path.basename(realCurrentDir);
    if (base === 'target' || base === 'node_modules' || base === '.git' || base === '.idea' || base === '.settings' || base === 'bin') {
      return;
    }

    let entries;
    try {
      entries = fs.readdirSync(realCurrentDir, { withFileTypes: true });
    } catch (err) {
      return;
    }

    for (const entry of entries) {
      const fullPath = path.join(realCurrentDir, entry.name);
      let isDir = entry.isDirectory();
      if (entry.isSymbolicLink()) {
        try {
          const stat = fs.statSync(fullPath);
          isDir = stat.isDirectory();
        } catch (e) {
          isDir = false;
        }
      }

      if (isDir) {
        walk(fullPath, depth + 1);
      } else {
        let isFile = entry.isFile();
        if (entry.isSymbolicLink()) {
          try {
            const stat = fs.statSync(fullPath);
            isFile = stat.isFile();
          } catch (e) {
            isFile = false;
          }
        }
        if (isFile) {
          if (entry.name.toLowerCase() === fileName.toLowerCase()) {
            const normalizedFullPath = fullPath.replace(/\\/g, '/').toLowerCase();
            const normalizedSuffix = suffix.replace(/\\/g, '/').toLowerCase();
            if (normalizedFullPath.endsWith(normalizedSuffix)) {
              foundPath = fullPath;
              return;
            }
          }
        }
      }
    }
  }
  walk(dir);
  return foundPath;
}

/**
 * Walk the filesystem for files matching the class name (e.g., UserController.java)
 * and check package declaration inside them.
 */
function findFileByClassNameAndPackage(dir, className, targetPackage) {
  const fileName = className + '.java';
  let foundPath = null;
  const visited = new Set();
  const MAX_DEPTH = 15;

  function walk(currentDir, depth = 0) {
    if (foundPath) return;
    if (depth > MAX_DEPTH) return;

    let realCurrentDir;
    try {
      realCurrentDir = fs.realpathSync(currentDir);
    } catch (err) {
      return;
    }

    if (visited.has(realCurrentDir)) return;
    visited.add(realCurrentDir);

    const base = path.basename(realCurrentDir);
    if (base === 'target' || base === 'node_modules' || base === '.git' || base === '.idea' || base === '.settings' || base === 'bin') {
      return;
    }

    let entries;
    try {
      entries = fs.readdirSync(realCurrentDir, { withFileTypes: true });
    } catch (err) {
      return;
    }

    for (const entry of entries) {
      const fullPath = path.join(realCurrentDir, entry.name);
      let isDir = entry.isDirectory();
      if (entry.isSymbolicLink()) {
        try {
          const stat = fs.statSync(fullPath);
          isDir = stat.isDirectory();
        } catch (e) {
          isDir = false;
        }
      }

      if (isDir) {
        walk(fullPath, depth + 1);
      } else {
        let isFile = entry.isFile();
        if (entry.isSymbolicLink()) {
          try {
            const stat = fs.statSync(fullPath);
            isFile = stat.isFile();
          } catch (e) {
            isFile = false;
          }
        }
        if (isFile && entry.name === fileName) {
          try {
            const content = fs.readFileSync(fullPath, 'utf8');
            const match = content.match(/^\s*package\s+([a-zA-Z0-9._]+);/m);
            if (match && match[1] === targetPackage) {
              foundPath = fullPath;
              return;
            }
          } catch (err) {
            // ignore
          }
        }
      }
    }
  }

  walk(dir);
  return foundPath;
}

/**
 * Split FQN and locate the class file by checking candidates
 */
function stripGenerics(typeStr) {
  if (!typeStr) return '';
  let result = '';
  let depth = 0;
  for (let i = 0; i < typeStr.length; i++) {
    const char = typeStr[i];
    if (char === '<') {
      depth++;
    } else if (char === '>') {
      depth = Math.max(0, depth - 1);
    } else if (depth === 0) {
      result += char;
    }
  }
  return result;
}

function locateJavaFileAndOuterClass(projectPath, dbPath, classFqn) {
  const cleanFqn = stripGenerics(classFqn);
  const normalizedFqn = cleanFqn.replace(/\$/g, '.');
  const parts = normalizedFqn.split('.');

  for (let i = parts.length - 1; i >= 0; i--) {
    const packageParts = parts.slice(0, i);
    const classParts = parts.slice(i);
    const candidateOuterClassFqn = parts.slice(0, i + 1).join('.');
    const packagePart = packageParts.join('.');
    const classPart = classParts[0];

    // 1. Try SQLite mapping helper
    let filePath = getPathFromDb(projectPath, dbPath, candidateOuterClassFqn);
    if (filePath && fs.existsSync(filePath)) {
      return {
        filePath,
        outerClassFqn: candidateOuterClassFqn,
        innerClasses: classParts.slice(1)
      };
    }

    // 2. Try direct convention paths
    const relSuffix = candidateOuterClassFqn.replace(/\./g, path.sep) + '.java';
    const mainPath = path.join(projectPath, 'src', 'main', 'java', relSuffix);
    if (fs.existsSync(mainPath)) {
      return {
        filePath: mainPath,
        outerClassFqn: candidateOuterClassFqn,
        innerClasses: classParts.slice(1)
      };
    }
    const testPath = path.join(projectPath, 'src', 'test', 'java', relSuffix);
    if (fs.existsSync(testPath)) {
      return {
        filePath: testPath,
        outerClassFqn: candidateOuterClassFqn,
        innerClasses: classParts.slice(1)
      };
    }

    // 3. Try fallback search using FQN structure
    filePath = findFileByFqn(projectPath, candidateOuterClassFqn);
    if (filePath && fs.existsSync(filePath)) {
      return {
        filePath,
        outerClassFqn: candidateOuterClassFqn,
        innerClasses: classParts.slice(1)
      };
    }

    // 4. Try class locating robustness
    if (packagePart) {
      filePath = findFileByClassNameAndPackage(projectPath, classPart, packagePart);
      if (filePath && fs.existsSync(filePath)) {
        return {
          filePath,
          outerClassFqn: candidateOuterClassFqn,
          innerClasses: classParts.slice(1)
        };
      }
    }
  }

  return null;
}

/**
 * Fetches workspace metadata from Rust backend to find project path and db path.
 */
async function getWorkspacePath(workspaceId) {
  try {
    const response = await axios.get(`${RUST_BACKEND_URL}/api/workspaces`);
    const workspace = response.data.find(w => w.id === workspaceId || w.id === parseInt(workspaceId, 10));
    return workspace ? { projectPath: workspace.project_path, dbPath: workspace.db_path } : null;
  } catch (error) {
    console.error(`[Symbol Resolver] Failed to fetch workspace path: ${error.message}`);
    return null;
  }
}

/**
 * Resolves a FQN symbol coordinates inside a project path.
 */
function resolveJavaSymbol(projectPath, dbPath, fqn) {
  const validFqnRegex = /^[a-zA-Z0-9_$.#(),[\]<>]+$/;
  if (!fqn || !validFqnRegex.test(fqn) || fqn.includes('..') || fqn.includes('/') || fqn.includes('\\')) {
    throw new Error(`Invalid symbol FQN: ${fqn}`);
  }

  const parsed = parseSymbolFQN(fqn);
  if (!parsed.classFqn) {
    throw new Error(`Invalid symbol FQN: ${fqn}`);
  }

  const located = locateJavaFileAndOuterClass(projectPath, dbPath, parsed.classFqn);
  if (!located || !located.filePath || !fs.existsSync(located.filePath)) {
    throw new Error(`Java source file not found for class: ${parsed.classFqn}`);
  }

  const { filePath, outerClassFqn, innerClasses } = located;

  // Path Traversal Mitigation: Ensure filePath is strictly a descendant of projectPath
  const absoluteProjectPath = fs.realpathSync(projectPath);
  const absoluteFilePath = fs.realpathSync(filePath);
  const relative = path.relative(absoluteProjectPath, absoluteFilePath);
  if (relative === '' || relative.startsWith('..') || path.isAbsolute(relative)) {
    throw new Error("Access denied: Path traversal detected");
  }

  // AST Parsing
  const content = fs.readFileSync(filePath, 'utf8');
  let cst;
  try {
    cst = javaParser.parse(content);
  } catch (parseError) {
    throw new Error(`Failed to parse Java file AST: ${parseError.message}`);
  }

  // Find class declaration node
  const outerClassName = outerClassFqn.substring(outerClassFqn.lastIndexOf('.') + 1);
  let classNode = findOuterClassNode(cst, outerClassName);
  if (!classNode) {
    throw new Error(`Outer class declaration not found: ${outerClassName}`);
  }

  // Walk into nested inner classes if any
  if (innerClasses.length > 0) {
    classNode = findNestedClassNode(classNode, innerClasses);
    if (!classNode) {
      throw new Error(`Inner class path not found: ${innerClasses.join('$')}`);
    }
  }

  // Case A: Resolving Class Only
  if (!parsed.methodName && !parsed.variableName) {
    const classIdToken = getDeclarationName(classNode);
    const range = getNodeRange(classIdToken || classNode);
    if (!range) throw new Error(`Could not determine location for class ${parsed.classFqn}`);
    return {
      filePath: path.resolve(filePath),
      ...range
    };
  }

  // Find method declaration node
  const classBodyNode = findClassBody(classNode);
  if (!classBodyNode) {
    throw new Error(`Could not find class body for: ${parsed.classFqn}`);
  }

  let methodNode = null;
  function searchMethods(node) {
    if (methodNode) return;
    if (!node || typeof node !== 'object') return;

    if (Array.isArray(node)) {
      node.forEach(searchMethods);
      return;
    }

    const matched = matchMethod(node, parsed.methodName, parsed.methodParams);
    if (matched) {
      methodNode = matched;
      return;
    }

    if (node.children) {
      for (const key in node.children) {
        // Do not cross into nested class declarations while searching methods
        if (
          key === 'classDeclaration' ||
          key === 'interfaceDeclaration' ||
          key === 'enumDeclaration' ||
          key === 'recordDeclaration' ||
          key === 'annotationTypeDeclaration'
        ) {
          continue;
        }
        searchMethods(node.children[key]);
      }
    }
  }

  searchMethods(classBodyNode);

  if (!methodNode) {
    throw new Error(`Method ${parsed.methodName} with matching signature not found in class ${parsed.classFqn}`);
  }

  // Case B: Resolving Method Only
  if (!parsed.variableName) {
    const methodIdToken = getMethodIdentifierNode(methodNode);
    const range = getNodeRange(methodIdToken || methodNode);
    if (!range) throw new Error(`Could not determine location for method ${parsed.methodName}`);
    return {
      filePath: path.resolve(filePath),
      ...range
    };
  }

  // Case C: Resolving Variable/Parameter
  const varNode = findVariableOrParameterNode(methodNode, parsed.variableName);
  if (!varNode) {
    throw new Error(`Variable/Parameter ${parsed.variableName} not found in method ${parsed.methodName}`);
  }

  const range = getNodeRange(varNode);
  if (!range) throw new Error(`Could not determine location for variable ${parsed.variableName}`);
  return {
    filePath: path.resolve(filePath),
    ...range
  };
}

module.exports = {
  getWorkspacePath,
  resolveJavaSymbol,
  parseSymbolFQN,
  findFileByFqn
};
