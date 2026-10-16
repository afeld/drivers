import { describe, it, expect, afterEach, vi } from 'vitest';
import ErrorCode from '../../src/constants/ErrorCode.js';
import ErrorMessage from '../../src/constants/ErrorMessage.js';
import { GlobalConfig, updateGlobalConfig, resetGlobalConfig } from '../../src/global-config.js';
import snowflake from '../../src/index.js';

describe('GlobalConfig', () => {
  afterEach(() => {
    resetGlobalConfig();
  });

  it('reads default config value', () => {
    expect(GlobalConfig.xmlParserConfig).toEqual({});
  });

  it('reads custom config value', () => {
    updateGlobalConfig({ xmlParserConfig: { ignoreAttributes: false } });
    expect(GlobalConfig.xmlParserConfig).toEqual({ ignoreAttributes: false });
  });

  describe('jsonColumnVariantParser', () => {
    it('parses standard JSON values', () => {
      expect(GlobalConfig.jsonColumnVariantParser('{"a":1}')).toEqual({ a: 1 });
      expect(GlobalConfig.jsonColumnVariantParser('[1,2,3]')).toEqual([1, 2, 3]);
      expect(GlobalConfig.jsonColumnVariantParser('"hello"')).toBe('hello');
    });

    it('parses Snowflake-specific non-JSON-compliant tokens via fallback', () => {
      expect(GlobalConfig.jsonColumnVariantParser('{"a":undefined}')).toEqual({ a: undefined });
      expect(GlobalConfig.jsonColumnVariantParser('{"a":NaN}').a).toBeNaN();
      expect(GlobalConfig.jsonColumnVariantParser('{"a":Infinity}')).toEqual({ a: Infinity });
    });

    it('invokes onNonJsonCompliantVariant only on the fallback path', () => {
      const onNonJsonCompliantVariant = vi.fn();
      // The context arg is intentionally omitted from the public CustomParser type
      // (see global-config.ts), so reach the runtime signature explicitly here.
      const parse = GlobalConfig.jsonColumnVariantParser as (
        rawColumnValue: string,
        context?: { onNonJsonCompliantVariant: () => void },
      ) => unknown;
      parse('{"a":1}', { onNonJsonCompliantVariant });
      expect(onNonJsonCompliantVariant).not.toHaveBeenCalled();
      parse('{"a":NaN}', { onNonJsonCompliantVariant });
      expect(onNonJsonCompliantVariant).toHaveBeenCalledTimes(1);
    });

    it('throws a SyntaxError on invalid JSON', () => {
      expect(() => GlobalConfig.jsonColumnVariantParser('{invalid')).toThrow(SyntaxError);
    });
  });

  describe('xmlColumnVariantParser', () => {
    it('parses valid XML using the default config', () => {
      expect(GlobalConfig.xmlColumnVariantParser('<root><a>1</a></root>')).toEqual({
        root: { a: 1 },
      });
    });

    it('throws on invalid XML', () => {
      expect(() => GlobalConfig.xmlColumnVariantParser('<root><a>1</root>')).toThrow();
    });

    it('honors xmlParserConfig when parsing attributes', () => {
      expect(GlobalConfig.xmlColumnVariantParser('<root a="1">text</root>')).toEqual({
        root: 'text',
      });
      updateGlobalConfig({ xmlParserConfig: { ignoreAttributes: false } });
      expect(GlobalConfig.xmlColumnVariantParser('<root a="1">text</root>')).toEqual({
        root: { '@_a': '1', '#text': 'text' },
      });
    });
  });

  describe('customCredentialManager', () => {
    const code = ErrorCode.ERR_GLOBAL_CONFIGURE_INVALID_CUSTOM_CREDENTIAL_MANAGER;

    it.each([
      ['a string', 'unsupported'],
      ['an empty object', {}],
      ['a manager missing read', { write() {}, remove() {} }],
      ['a manager missing write', { read() {}, remove() {} }],
      ['a manager missing remove', { read() {}, write() {} }],
    ])('should reject %s', (_, manager) => {
      expect(() => snowflake.configure({ customCredentialManager: manager as never })).toThrow(
        expect.objectContaining({
          name: 'InvalidParameterError',
          code,
          message: ErrorMessage[code],
        }),
      );
    });

    it('should accept an object with the required callbacks', () => {
      expect(() =>
        snowflake.configure({
          customCredentialManager: {
            read: () => null,
            write: () => undefined,
            remove: () => undefined,
          },
        }),
      ).not.toThrow();
    });
  });
});
