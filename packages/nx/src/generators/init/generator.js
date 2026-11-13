'use strict';

const PLUGIN = 'nx-scopetest';
const CACHE_DIR = '.scopetest';

/** Registers the plugin in nx.json and git-ignores the scopetest cache. */
module.exports = async function initGenerator(tree, options = {}) {
  const targetName = options.targetName || 'test-affected';

  const nxJson = JSON.parse(tree.read('nx.json', 'utf-8'));
  nxJson.plugins = nxJson.plugins || [];
  const registered = nxJson.plugins.some(
    (entry) => (typeof entry === 'string' ? entry : entry.plugin) === PLUGIN
  );
  if (!registered) {
    nxJson.plugins.push({ plugin: PLUGIN, options: { targetName } });
    tree.write('nx.json', `${JSON.stringify(nxJson, null, 2)}\n`);
  }

  const gitignore = tree.exists('.gitignore') ? tree.read('.gitignore', 'utf-8') : '';
  const ignored = gitignore.split(/\r?\n/).some((line) => line.trim().replace(/\/$/, '') === CACHE_DIR);
  if (!ignored) {
    const separator = gitignore && !gitignore.endsWith('\n') ? '\n' : '';
    tree.write('.gitignore', `${gitignore}${separator}${CACHE_DIR}/\n`);
  }
};
