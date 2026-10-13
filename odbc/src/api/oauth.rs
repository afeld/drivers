//! Registry-driven secret policies for the ODBC wrapper: never persist
//! secrets to the DSN registry ([`should_persist_to_dsn`]) and redact them
//! from logs ([`redacted_param_map`]). Both delegate sensitivity and semantic
//! type to `sf_core`'s parameter registry under the ODBC flavor, covering
//! OAuth and legacy PWD-family secrets in one place.

/// Returns `false` for every key the parameter registry marks sensitive, so no
/// secret reaches the on-disk DSN registry. Keys unknown to the registry are
/// persisted, so callers compose this with their own DSN-skip rules (`DSN`).
/// Case-insensitive in `key`.
#[allow(dead_code)] // consumed by setup_common.rs (windows-only DSN write path)
pub fn should_persist_to_dsn(key: &str) -> bool {
    use sf_core::config::param_registry::{Wrapper, registry};
    !registry().is_sensitive_for(Wrapper::Odbc, key)
}

const CONSERVATIVE_LOG_REDACT: &[&str] = &["OAUTH_CLIENT_ID"];

/// Returns a borrowed view of `params` with each value sanitized for logging.
///
/// URL-typed parameters (proxy, server URL, authenticator, OAuth/WIF endpoint
/// URIs) have their password masked and query strings and fragments stripped,
/// but scheme, user, host, port, and path are preserved for diagnostics.
/// A scheme-bearing value with a colon and no `@` is fully masked, except a
/// dotted host or bracketed IPv6 with a numeric port. Other sensitive
/// parameters are fully masked with `"****"`.
/// Everything else is passed through unchanged.
///
/// Use this at every connection-string logging boundary so secrets never reach
/// `tracing` sinks.
pub fn redacted_param_map(
    params: &std::collections::HashMap<String, String>,
) -> std::collections::HashMap<&String, std::borrow::Cow<'_, str>> {
    use sf_core::config::param_registry::{LogicalType, Wrapper, registry};
    use sf_core::logging::sanitize_config_url_for_log;
    let registry = registry();
    params
        .iter()
        .map(|(k, v)| {
            let value: std::borrow::Cow<'_, str> = match registry.logical_type_for(Wrapper::Odbc, k)
            {
                LogicalType::Url => sanitize_config_url_for_log(v),
                LogicalType::Unspecified => {
                    if registry.is_sensitive_for(Wrapper::Odbc, k)
                        || CONSERVATIVE_LOG_REDACT
                            .iter()
                            .any(|rk| k.eq_ignore_ascii_case(rk))
                    {
                        std::borrow::Cow::Borrowed("****")
                    } else {
                        std::borrow::Cow::Borrowed(v.as_str())
                    }
                }
            };
            (k, value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_persist_to_dsn_returns_false_only_for_secrets() {
        assert!(!should_persist_to_dsn("OAUTH_CLIENT_SECRET"));
        assert!(!should_persist_to_dsn("oauth_client_secret"));
        assert!(!should_persist_to_dsn("TOKEN"));
        assert!(!should_persist_to_dsn("PROXY"));
        assert!(should_persist_to_dsn("OAUTH_CLIENT_ID"));
        assert!(should_persist_to_dsn("OAUTH_REDIRECT_URI"));
        assert!(should_persist_to_dsn("OAUTH_SCOPE"));
        assert!(should_persist_to_dsn("UID"));
    }

    #[test]
    fn redacted_param_map_redacts_legacy_and_oauth_secrets() {
        use std::collections::HashMap;
        let params: HashMap<String, String> = [
            ("UID", "joe"),
            ("PWD", "hunter2"),
            ("PASSWORD", "hunter3"),
            ("PRIV_KEY_FILE_PWD", "kpwd"),
            ("PRIV_KEY_PWD", "kpwd2"),
            ("PRIV_KEY_BASE64", "AAA="),
            ("PASSCODE", "123456"),
            ("OAUTH_CLIENT_ID", "abc"),
            ("OAUTH_CLIENT_SECRET", "shhh"),
            ("TOKEN", "jwt.value"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

        let redacted = redacted_param_map(&params);

        assert_eq!(
            redacted.get(&"UID".to_owned()).map(|v| v.as_ref()),
            Some("joe")
        );
        for sensitive in [
            "PWD",
            "PASSWORD",
            "PRIV_KEY_FILE_PWD",
            "PRIV_KEY_PWD",
            "PRIV_KEY_BASE64",
            "PASSCODE",
            "OAUTH_CLIENT_ID",
            "OAUTH_CLIENT_SECRET",
            "TOKEN",
        ] {
            let key = sensitive.to_string();
            assert_eq!(
                redacted.get(&key).map(|v| v.as_ref()),
                Some("****"),
                "expected key {sensitive} to be redacted"
            );
        }
    }

    #[test]
    fn redacted_param_map_redaction_is_case_insensitive() {
        use std::collections::HashMap;
        let params: HashMap<String, String> = [
            ("oauth_client_secret", "shhh"),
            ("Pwd", "hunter2"),
            ("Password", "hunter3"),
            ("token", "jwt.value"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

        let redacted = redacted_param_map(&params);
        for k in params.keys() {
            assert_eq!(
                redacted.get(k).map(|v| v.as_ref()),
                Some("****"),
                "expected key {k} to be redacted regardless of case"
            );
        }
    }

    #[test]
    fn redacted_param_map_masks_proxy_password() {
        use std::collections::HashMap;
        let params: HashMap<String, String> = [
            ("PROXY_PASSWORD", "DO_NOT_LEAK_PROXY_SECRET_42"),
            ("Proxy_Password", "DO_NOT_LEAK_PROXY_SECRET_42"),
            ("proxy_password", "DO_NOT_LEAK_PROXY_SECRET_42"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

        let redacted = redacted_param_map(&params);
        for k in params.keys() {
            assert_eq!(
                redacted.get(k).map(|v| v.as_ref()),
                Some("****"),
                "PROXY_PASSWORD variant {k:?} must be fully redacted"
            );
        }
        let debug = format!("{redacted:?}");
        assert!(
            !debug.contains("DO_NOT_LEAK_PROXY_SECRET_42"),
            "canary password leaked in Debug output: {debug}"
        );
    }

    #[test]
    fn redacted_param_map_strips_proxy_url_password() {
        use std::collections::HashMap;
        let params: HashMap<String, String> = [(
            "PROXY",
            "http://proxyuser:DO_NOT_LEAK_PROXY_SECRET_42@proxy.example.com:8080",
        )]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

        let redacted = redacted_param_map(&params);
        let val = redacted
            .get(&"PROXY".to_owned())
            .expect("PROXY key missing");

        assert!(
            !val.contains("DO_NOT_LEAK_PROXY_SECRET_42"),
            "canary password leaked in PROXY value: {val}"
        );
        assert!(
            val.contains("proxy.example.com"),
            "host must remain visible for diagnostics: {val}"
        );
        assert!(
            val.contains("proxyuser"),
            "username should remain visible: {val}"
        );
        assert_eq!(val.as_ref(), "http://proxyuser:****@proxy.example.com:8080");

        let debug = format!("{redacted:?}");
        assert!(
            !debug.contains("DO_NOT_LEAK_PROXY_SECRET_42"),
            "canary password leaked in Debug output: {debug}"
        );
    }

    #[test]
    fn redacted_param_map_strips_proxy_url_password_case_insensitive() {
        use std::collections::HashMap;
        let params: HashMap<String, String> =
            [("proxy", "https://u:DO_NOT_LEAK_PROXY_SECRET_42@h:443")]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect();

        let redacted = redacted_param_map(&params);
        let val = redacted
            .get(&"proxy".to_owned())
            .expect("proxy key missing");
        assert_eq!(val.as_ref(), "https://u:****@h:443");
    }

    #[test]
    fn redacted_param_map_proxy_url_without_password_unchanged() {
        use std::collections::HashMap;
        let params: HashMap<String, String> = [
            ("PROXY", "http://proxy.example.com:8080"),
            ("PROXY_HOST", "proxy.example.com"),
            ("PROXY_USER", "proxyuser"),
            ("PROXY_PORT", "8080"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

        let redacted = redacted_param_map(&params);
        assert_eq!(
            redacted.get(&"PROXY".to_owned()).map(|v| v.as_ref()),
            Some("http://proxy.example.com:8080"),
            "PROXY URL without userinfo must remain visible"
        );
        assert_eq!(
            redacted.get(&"PROXY_HOST".to_owned()).map(|v| v.as_ref()),
            Some("proxy.example.com"),
            "PROXY_HOST is not sensitive"
        );
        assert_eq!(
            redacted.get(&"PROXY_USER".to_owned()).map(|v| v.as_ref()),
            Some("proxyuser"),
            "PROXY_USER is not sensitive"
        );
        assert_eq!(
            redacted.get(&"PROXY_PORT".to_owned()).map(|v| v.as_ref()),
            Some("8080"),
            "PROXY_PORT is not sensitive"
        );
    }

    #[test]
    fn redacted_param_map_proxy_url_user_only_no_password() {
        use std::collections::HashMap;
        let params: HashMap<String, String> = [("PROXY", "http://justuser@proxy.example.com:3128")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();

        let redacted = redacted_param_map(&params);
        assert_eq!(
            redacted.get(&"PROXY".to_owned()).map(|v| v.as_ref()),
            Some("http://justuser@proxy.example.com:3128"),
            "user-only userinfo (no colon) has no password to strip"
        );
    }

    #[test]
    fn redacted_param_map_proxy_url_slash_in_password_fails_closed() {
        use std::collections::HashMap;
        let canary = "DO_NOT_LEAK_PROXY_SECRET_42";
        let params: HashMap<String, String> = [
            ("PROXY", format!("http://user:pa/{canary}@host:8080")),
            ("proxy", format!("http://user:p@ss/{canary}@host")),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

        let redacted = redacted_param_map(&params);
        let debug = format!("{redacted:?}");
        assert!(
            !debug.contains(canary),
            "canary password leaked for slash-in-password URL: {debug}"
        );
        for k in params.keys() {
            assert_eq!(
                redacted.get(k).map(|v| v.as_ref()),
                Some("****"),
                "{k}: ambiguous URL must fail closed to ****"
            );
        }
    }

    #[test]
    fn redacted_param_map_oauth_url_query_is_stripped() {
        use std::collections::HashMap;
        let params: HashMap<String, String> = [
            (
                "OAUTH_AUTHORIZATION_URL",
                "https://idp.example.com/auth?client_id=abc&redirect_uri=http://localhost",
            ),
            (
                "OAUTH_TOKEN_REQUEST_URL",
                "https://idp.example.com/token?audience=snowflake",
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

        let redacted = redacted_param_map(&params);
        assert_eq!(
            redacted
                .get(&"OAUTH_AUTHORIZATION_URL".to_owned())
                .map(|v| v.as_ref()),
            Some("https://idp.example.com/auth"),
            "query string on OAUTH_AUTHORIZATION_URL must be stripped"
        );
        assert_eq!(
            redacted
                .get(&"OAUTH_TOKEN_REQUEST_URL".to_owned())
                .map(|v| v.as_ref()),
            Some("https://idp.example.com/token"),
            "query string on OAUTH_TOKEN_REQUEST_URL must be stripped"
        );
    }

    #[test]
    fn redacted_param_map_server_url_with_embedded_credentials_is_sanitized() {
        use std::collections::HashMap;
        let canary = "DO_NOT_LEAK_SERVER_SECRET_42";
        let params: HashMap<String, String> = [(
            "SERVER_URL",
            format!("https://user:{canary}@account.snowflakecomputing.com/"),
        )]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

        let redacted = redacted_param_map(&params);
        let val = redacted
            .get(&"SERVER_URL".to_owned())
            .expect("SERVER_URL key missing");
        assert!(
            !val.contains(canary),
            "canary password leaked in SERVER_URL: {val}"
        );
        assert!(
            val.contains("account.snowflakecomputing.com"),
            "host must remain visible: {val}"
        );
    }

    #[test]
    fn redacted_param_map_authenticator_okta_url_strips_query() {
        use std::collections::HashMap;
        let canary = "DO_NOT_LEAK_CLIENT_ID_42";
        let params: HashMap<String, String> = [(
            "AUTHENTICATOR",
            format!("https://idp.example.com/app?client_id={canary}"),
        )]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

        let redacted = redacted_param_map(&params);
        assert_eq!(
            redacted
                .get(&"AUTHENTICATOR".to_owned())
                .map(|v| v.as_ref()),
            Some("https://idp.example.com/app"),
            "Okta authenticator URL keeps host and path and drops the query"
        );
        let debug = format!("{redacted:?}");
        assert!(
            !debug.contains(canary),
            "client_id leaked from AUTHENTICATOR: {debug}"
        );
    }

    #[test]
    fn redacted_param_map_oauth_redirect_uri_strips_query() {
        use std::collections::HashMap;
        let canary = "DO_NOT_LEAK_REDIRECT_CODE_42";
        let params: HashMap<String, String> = [(
            "OAUTH_REDIRECT_URI",
            format!("http://127.0.0.1:0/cb?code={canary}"),
        )]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

        let redacted = redacted_param_map(&params);
        assert_eq!(
            redacted
                .get(&"OAUTH_REDIRECT_URI".to_owned())
                .map(|v| v.as_ref()),
            Some("http://127.0.0.1:0/cb"),
            "OAUTH_REDIRECT_URI keeps host, port, and path and drops the query"
        );
        let debug = format!("{redacted:?}");
        assert!(
            !debug.contains(canary),
            "query leaked from OAUTH_REDIRECT_URI: {debug}"
        );
    }

    #[test]
    fn redacted_param_map_proxy_url_missing_at_fails_closed() {
        use std::collections::HashMap;
        let canary = "DO_NOT_LEAK_PROXY_SECRET_42";
        let params: HashMap<String, String> = [("PROXY", format!("https://user:{canary}?token=x"))]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();

        let redacted = redacted_param_map(&params);
        assert_eq!(
            redacted.get(&"PROXY".to_owned()).map(|v| v.as_ref()),
            Some("****"),
            "PROXY URL with a password colon and no @ must fail closed"
        );
        let debug = format!("{redacted:?}");
        assert!(
            !debug.contains(canary),
            "password leaked from PROXY URL with no @: {debug}"
        );
    }

    #[test]
    fn redacted_param_map_authenticator_keyword_unchanged() {
        use std::collections::HashMap;
        let params: HashMap<String, String> = [
            ("AUTHENTICATOR", "externalbrowser"),
            ("authenticator", "snowflake"),
            ("Authenticator", "oauth"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

        let redacted = redacted_param_map(&params);
        assert_eq!(
            redacted
                .get(&"AUTHENTICATOR".to_owned())
                .map(|v| v.as_ref()),
            Some("externalbrowser")
        );
        assert_eq!(
            redacted
                .get(&"authenticator".to_owned())
                .map(|v| v.as_ref()),
            Some("snowflake")
        );
        assert_eq!(
            redacted
                .get(&"Authenticator".to_owned())
                .map(|v| v.as_ref()),
            Some("oauth")
        );
    }

    #[test]
    fn redacted_param_map_uid_with_at_sign_unchanged() {
        use std::collections::HashMap;
        let params: HashMap<String, String> = [("UID", "alice@example.com")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();

        let redacted = redacted_param_map(&params);
        assert_eq!(
            redacted.get(&"UID".to_owned()).map(|v| v.as_ref()),
            Some("alice@example.com"),
            "UID is not URL-typed; @ in the value must not be treated as a URL delimiter"
        );
    }
}
