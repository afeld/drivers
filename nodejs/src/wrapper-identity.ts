import type { WrapperIdentity } from './core/binary-types.generated.js';

const { version } = require('../package.json') as { version: string };

type RuntimeVersions = NodeJS.ProcessVersions & {
  bun?: string;
  deno?: string;
};

/**
 * Returns the file path of the main application entry point.
 *
 * - Uses require.main.filename (CommonJS) as primary source
 * - Falls back to process.argv[1] for ESM or worker threads
 * - Returns undefined in REPL or bundlers
 */
function applicationPath(): string | undefined {
  return require.main?.filename || process.argv[1];
}

function languageIdentity(): { languageRuntime: string; languageVersion: string } {
  const versions = process.versions as RuntimeVersions;
  if (versions.deno) {
    return { languageRuntime: 'deno', languageVersion: versions.deno };
  }
  if (versions.bun) {
    return { languageRuntime: 'bun', languageVersion: versions.bun };
  }
  return { languageRuntime: 'nodejs', languageVersion: versions.node };
}

export function getWrapperIdentity(): WrapperIdentity {
  const { languageRuntime, languageVersion } = languageIdentity();
  return {
    driverName: 'JavaScript',
    driverVersion: version,
    languageRuntime,
    languageVersion,
    applicationPath: applicationPath(),
  };
}
