use std::sync::Arc;

use serde_json::json;
use sf_core::fs_adapter::RealFs;
use sf_core::logging::LogManager;
use sf_core::protobuf::apis::database_driver_v1::{DriverProviders, WrapperPresets};
use sf_core::telemetry::platform_detection::platform_detection_env_vars;
use sf_core::telemetry::{SESSION_ID_FIELD, TELEMETRY_ID_FIELD};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::common::decompress_gzip_json;
use crate::common::snowflake_test_client::SnowflakeTestClient;
use crate::common::tls_proxy::MockServerWithTls;

#[test]
fn should_send_telemetry_of_an_adopted_session_with_its_token_and_without_a_session_id() {
    let detection_disabled =
        platform_detection_env_vars(&[("SNOWFLAKE_DISABLE_PLATFORM_DETECTION", "true")]);
    temp_env::with_vars(detection_disabled, || {
        let mock = MockServerWithTls::start();
        mock.mount(
            Mock::given(method("POST"))
                .and(path("/telemetry/send"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true}))),
        );
        mock.mount(
            Mock::given(method("POST"))
                .and(path("/session"))
                .and(query_param("delete", "true"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success": true}))),
        );
        let providers = DriverProviders {
            log_manager: Some(LogManager::with_none_subscriber(Arc::new(RealFs))),
            wrapper_presets: WrapperPresets {
                validate_session_token: false,
                ..WrapperPresets::default()
            },
            ..Default::default()
        };
        let client =
            SnowflakeTestClient::with_int_tests_params_using(Some(&mock.http_url()), providers);
        client.set_connection_option("session_token", "adopted-session-token");
        client.set_connection_option("master_token", "adopted-master-token");

        client.connect().expect("adopting tokens should succeed");
        assert!(
            mock.received_requests().is_empty(),
            "adopting tokens must not reach the server"
        );
        client
            .connection_close_blocking()
            .expect("close should succeed");

        let sends: Vec<_> = mock
            .received_requests()
            .into_iter()
            .filter(|req| req.url.path() == "/telemetry/send")
            .collect();
        assert_eq!(sends.len(), 1, "one batch flushed on close");
        let authorization = sends[0]
            .headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert!(
            authorization.contains("adopted-session-token"),
            "batch must be authorized by the adopted session token, got {authorization:?}"
        );
        let body = decompress_gzip_json(&sends[0].body);
        let logs = body["logs"].as_array().expect("logs array");
        assert!(
            logs.iter()
                .any(|entry| entry["message"]["type"] == "platform_detection_status"),
            "flushed batch must carry the connection's entries, got {body}"
        );
        for entry in logs {
            assert!(entry["message"].get(SESSION_ID_FIELD).is_none(), "{entry}");
            assert!(
                entry["message"].get(TELEMETRY_ID_FIELD).is_none(),
                "{entry}"
            );
        }
    });
}
