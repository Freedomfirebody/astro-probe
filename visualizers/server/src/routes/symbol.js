const express = require('express');
const router = express.Router({ mergeParams: true });
const { getWorkspacePath, resolveJavaSymbol } = require('../services/symbolResolver');

router.get('/', async (req, res) => {
  const { id } = req.params;
  const { fqn } = req.query;

  if (!fqn) {
    return res.status(400).json({ error: 'Missing parameter fqn' });
  }

  const validFqnRegex = /^[a-zA-Z0-9_$.#(),[\]<>]+$/;
  if (!validFqnRegex.test(fqn) || fqn.includes('..') || fqn.includes('/') || fqn.includes('\\')) {
    return res.status(400).json({ error: 'Invalid symbol FQN' });
  }

  try {
    const workspaceInfo = await getWorkspacePath(id);
    if (!workspaceInfo) {
      return res.status(404).json({ error: `Workspace with ID ${id} not found on backend` });
    }
    const { projectPath, dbPath } = workspaceInfo;
 
    const symbolLocation = resolveJavaSymbol(projectPath, dbPath, fqn);
    res.json(symbolLocation);
  } catch (error) {
    console.error(`[Symbol Route Error] FAILED resolving fqn '${fqn}':`, error.message);
    res.status(404).json({ error: error.message });
  }
});

module.exports = router;
