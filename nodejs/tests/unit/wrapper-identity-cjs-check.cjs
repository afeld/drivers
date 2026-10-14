const assert = require('node:assert');

async function main() {
  process.argv[1] = '/not/the/entry.js';
  const { getWrapperIdentity } = await import('../../src/wrapper-identity.ts');
  assert.strictEqual(getWrapperIdentity().applicationPath, require.main.filename);
}

main();
