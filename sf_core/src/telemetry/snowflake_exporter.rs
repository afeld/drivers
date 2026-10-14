use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;

use opentelemetry::KeyValue;
use opentelemetry_sdk::error::OTelSdkResult;
use tokio::sync::RwLock as AsyncRwLock;

use crate::config::rest_parameters::QueryParameters;
use crate::log_foreign_error;
use crate::rest::snowflake::SessionTokens;
use crate::rest::snowflake::telemetry as rest;

use super::TELEMETRY_ID_FIELD;

/// Shared registry mapping connection telemetry ids to their exporter sessions.
/// Connections register on init, deregister on close.
pub type SessionRegistry = Arc<RwLock<HashMap<u64, Arc<ExporterSession>>>>;

/// Shared session context used to POST telemetry to Snowflake's in-band
/// `/telemetry/send` endpoint. Both telemetry producers (span lane, raw-log
/// lane) look this up by telemetry id via [`SessionRegistry`] and egress through
/// [`send_with_token`], so they share one wire path and one auth path.
pub struct ExporterSession {
    pub client: reqwest::Client,
    pub query_parameters: QueryParameters,
    pub session_token: Arc<AsyncRwLock<Option<SessionTokens>>>,
}

impl ExporterSession {
    /// The session id from the connection's current tokens. `None` for a
    /// connection that adopted session tokens without learning one.
    pub(crate) async fn session_id(&self) -> Option<i64> {
        self.session_token
            .read()
            .await
            .as_ref()
            .and_then(|tokens| tokens.session_id)
    }
}

impl std::fmt::Debug for ExporterSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExporterSession")
            .field("server_url", &self.query_parameters.server_url)
            .finish_non_exhaustive()
    }
}

/// Clone the session token under a short-lived read guard, then send
/// telemetry without holding the lock across the network call. Shared by both
/// telemetry lanes so both egress through one path. All errors are non-fatal —
/// telemetry must never break the user's workflow.
pub(crate) async fn send_with_token(
    session: &ExporterSession,
    payload: &serde_json::Value,
) -> OTelSdkResult {
    let token = {
        let guard = session.session_token.read().await;
        match guard.as_ref() {
            Some(tokens) => tokens.session_token.clone(),
            None => {
                tracing::debug!("No active session token, dropping telemetry");
                return Ok(());
            }
        }
    };

    if let Err(e) = rest::send_telemetry(
        &session.client,
        &session.query_parameters,
        token.reveal(),
        payload,
    )
    .await
    {
        log_foreign_error!(warn, e, "Failed to export telemetry");
    }

    // Best-effort: always return Ok
    Ok(())
}

/// Extract the `snowflake.telemetry.id` attribute value from a span's
/// attributes. A `u64` tracing field reaches OpenTelemetry as a String
/// (`tracing-opentelemetry` records it through its Debug fallback); I64 is
/// accepted for attributes built directly.
pub(crate) fn extract_telemetry_id(attrs: &[KeyValue]) -> Option<u64> {
    use opentelemetry::Value;
    attrs.iter().find_map(|kv| {
        if kv.key.as_str() == TELEMETRY_ID_FIELD {
            match &kv.value {
                Value::I64(id) => u64::try_from(*id).ok(),
                Value::String(s) => s.as_str().parse::<u64>().ok(),
                _ => None,
            }
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_telemetry_id_handles_i64_value() {
        let attrs = vec![KeyValue::new(TELEMETRY_ID_FIELD, 42i64)];
        assert_eq!(extract_telemetry_id(&attrs), Some(42));
    }

    #[test]
    fn extract_telemetry_id_handles_string_value() {
        let attrs = vec![KeyValue::new(TELEMETRY_ID_FIELD, "99")];
        assert_eq!(extract_telemetry_id(&attrs), Some(99));
    }

    #[test]
    fn extract_telemetry_id_returns_none_when_missing() {
        let attrs = vec![KeyValue::new("other.attr", "value")];
        assert_eq!(extract_telemetry_id(&attrs), None);
    }
}
