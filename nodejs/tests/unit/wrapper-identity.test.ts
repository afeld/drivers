import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { getWrapperIdentity } from '../../src/wrapper-identity.js';

describe('getWrapperIdentity', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  function stubVersions(extra: { bun?: string; deno?: string }) {
    const versions = { ...process.versions, ...extra };
    vi.spyOn(process, 'versions', 'get').mockReturnValue(versions);
  }

  it('should carry the driver name, package version, and Node runtime', () => {
    const { version } = createRequire(import.meta.url)('../../package.json') as { version: string };
    const identity = getWrapperIdentity();

    expect(identity.driverName).toBe('JavaScript');
    expect(identity.driverVersion).toBe(version);
    expect(identity.languageRuntime).toBe('nodejs');
    expect(identity.languageVersion).toBe(process.versions.node);
  });

  it('should report Deno when process.versions.deno is set', () => {
    stubVersions({ deno: '2.1.4', bun: '1.2.3' });
    const identity = getWrapperIdentity();

    expect(identity.languageRuntime).toBe('deno');
    expect(identity.languageVersion).toBe('2.1.4');
  });

  it('should report Bun when process.versions.bun is set', () => {
    stubVersions({ bun: '1.2.3' });
    const identity = getWrapperIdentity();

    expect(identity.languageRuntime).toBe('bun');
    expect(identity.languageVersion).toBe('1.2.3');
  });

  it('should fall back to the stubbed process.argv[1] when there is no CommonJS main module', () => {
    vi.spyOn(process, 'argv', 'get').mockReturnValue(['node', 'filename.js']);

    expect(getWrapperIdentity().applicationPath).toBe('filename.js');
  });

  it('should leave the application path unset when argv has no script and there is no CommonJS main module', () => {
    vi.spyOn(process, 'argv', 'get').mockReturnValue(['node']);

    expect(getWrapperIdentity().applicationPath).toBeUndefined();
  });

  it('should prefer require.main.filename over process.argv[1]', () => {
    execFileSync(
      process.execPath,
      [
        '--experimental-strip-types',
        fileURLToPath(new URL('./wrapper-identity-cjs-check.cjs', import.meta.url)),
      ],
      { stdio: 'pipe' },
    );
  });
});
