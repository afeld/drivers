import { XMLParser, XMLValidator, X2jOptions } from 'fast-xml-parser';
import ErrorCode from './constants/ErrorCode.js';
import ErrorMessage from './constants/ErrorMessage.js';
import { CoreSetCustomCredentialManager } from './core/index.js';
import { invalidParameterError } from './error.js';

export type CustomParser = (rawColumnValue: string) => any;
export type XMlParserConfigOption = X2jOptions;

/** Token store; methods may return a value or a Promise that settles within 30 seconds. */
export interface CustomCredentialManager {
  read(key: string): string | null | undefined | Promise<string | null | undefined>;
  write(key: string, token: string): unknown | Promise<unknown>;
  remove(key: string): unknown | Promise<unknown>;
}

export interface ConfigureOptions {
  /**
   * Custom parser for JSON data in VARIANT, OBJECT, and ARRAY columns.
   *
   * By default the driver parses values with `JSON.parse()`. If that fails (e.g. the
   * value contains non-standard tokens like `undefined`, `NaN`, or `Infinity` that
   * Snowflake's VARIANT type allows), it falls back to eval-based parsing, which is
   * slower and logs a warning.
   *
   * To avoid the fallback, set the `STRICT_JSON_OUTPUT` session parameter to `TRUE` so
   * Snowflake normalizes non-standard values into valid JSON before sending them.
   *
   * @see https://docs.snowflake.com/en/developer-guide/node-js/nodejs-driver-consume
   * @see https://docs.snowflake.com/en/sql-reference/parameters#strict-json-output
   */
  jsonColumnVariantParser?: CustomParser;

  /**
   * Custom parser for XML data in VARIANT columns.
   *
   * The driver always attempts JSON parsing first for every VARIANT value. Only when
   * JSON parsing fails does it try this XML parser, so XML values always incur the
   * overhead of a failed JSON parse attempt before being handled.
   *
   * The built-in parser uses `fast-xml-parser` and ignores XML attributes by default.
   * Use `xmlParserConfig` to customize attribute handling.
   *
   * @see https://docs.snowflake.com/en/developer-guide/node-js/nodejs-driver-consume
   */
  xmlColumnVariantParser?: CustomParser;

  /**
   * Configuration passed through to `fast-xml-parser`, the library backing the
   * built-in `xmlColumnVariantParser`.
   *
   * These options are forwarded directly to the `XMLParser` constructor, so they
   * control how XML in VARIANT columns is parsed (e.g. attribute handling, tag value
   * processing). See `fast-xml-parser`'s `X2jOptions` for the full set of options.
   *
   * Has no effect if you supply your own `xmlColumnVariantParser`.
   */
  xmlParserConfig?: XMlParserConfigOption;

  /**
   * Whether the driver reads proxy settings from the `HTTP_PROXY`, `HTTPS_PROXY`, and `NO_PROXY`
   * environment variables when a connection does not configure an explicit proxy. An explicit
   * `proxyHost`/`proxyPort` always takes precedence over the environment.
   *
   * @default true
   */
  useEnvProxy?: boolean;

  /**
   * Token store that replaces the OS keyring for cached credentials.
   *
   * Once registered, it is used by default for MFA, external browser, and OAuth
   * Authorization Code logins, because `clientStoreTemporaryCredential` defaults to `true`.
   * A connection that sets `clientStoreTemporaryCredential: false` never calls it.
   *
   * It receives ID tokens, MFA tokens, OAuth access and refresh tokens, and DPoP private keys.
   */
  customCredentialManager?: CustomCredentialManager;
}

type ResolvedConfig = Required<Omit<ConfigureOptions, 'customCredentialManager'>>;

const buildDefaults = (): ResolvedConfig => ({
  jsonColumnVariantParser: (
    rawColumnValue: string,
    // NOTE: We're intentionally not documenting the context argument in CustomParser to match old driver typing
    context?: {
      onNonJsonCompliantVariant: () => void;
    },
  ) => {
    try {
      return JSON.parse(rawColumnValue);
    } catch {
      // NOTE:
      // JSON.parse (RFC 8259) throws on values such as undefined, NaN, and Infinity,
      // but Snowflake can return them when STRICT_JSON_OUTPUT is FALSE (default).
      // This fallback evaluates only VARIANT output from the trusted Snowflake service,
      // whose encoder does not emit executable JavaScript.
      //
      // https://docs.snowflake.com/en/sql-reference/parameters#strict_json_output
      const result = new Function(`return (${rawColumnValue});`)();
      context?.onNonJsonCompliantVariant();
      return result;
    }
  },
  xmlColumnVariantParser: (rawColumnValue: string) => {
    const validateResult = XMLValidator.validate(rawColumnValue);
    if (validateResult === true) {
      return new XMLParser(GlobalConfig.xmlParserConfig).parse(rawColumnValue);
    } else {
      throw new Error(validateResult.err.msg);
    }
  },
  xmlParserConfig: {},
  useEnvProxy: true,
});

export const GlobalConfig: ResolvedConfig = {
  ...buildDefaults(),
};

export function updateGlobalConfig({
  customCredentialManager,
  ...options
}: Partial<ConfigureOptions>) {
  if (customCredentialManager != null) {
    setCustomCredentialManager(customCredentialManager);
  }
  Object.assign(GlobalConfig, options);
}

function setCustomCredentialManager(manager: CustomCredentialManager) {
  const isValid =
    typeof manager === 'object' &&
    (['read', 'write', 'remove'] as const).every(
      (method) => Object.hasOwn(manager, method) && typeof manager[method] === 'function',
    );
  if (!isValid) {
    const code = ErrorCode.ERR_GLOBAL_CONFIGURE_INVALID_CUSTOM_CREDENTIAL_MANAGER;
    throw invalidParameterError(ErrorMessage[code], code);
  }
  CoreSetCustomCredentialManager({
    read: async (key) => manager.read(key),
    write: async (key, token) => {
      await manager.write(key, token);
    },
    remove: async (key) => {
      await manager.remove(key);
    },
  });
}

export const resetGlobalConfig = () => {
  Object.assign(GlobalConfig, buildDefaults());
  CoreSetCustomCredentialManager(null);
};
