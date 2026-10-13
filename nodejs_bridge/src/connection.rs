use crate::BRIDGE;
use crate::error::{BridgeError, ToJsError, async_to_js};
use crate::query;
use crate::query_status::QueryStatus;
use crate::session::Session;
use crate::session_params::KnownSessionParameters;
use crate::statement::{ExecuteParams, Statement};
use napi::bindgen_prelude::*;
use napi::threadsafe_function::ThreadsafeFunctionCallMode;
use napi_derive::napi;
use sf_core::apis::database_driver_v1::ApiError;
use sf_core::apis::database_driver_v1::connection::WrapperIdentity;
use sf_core::config::param_names;
use sf_core::config::rest_parameters::BrowserOpenFn;
use sf_core::config::settings::Setting;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const NODE_TLS_REJECT_UNAUTHORIZED: &str = "NODE_TLS_REJECT_UNAUTHORIZED";
const NODE_EXTRA_CA_CERTS: &str = "NODE_EXTRA_CA_CERTS";

#[napi(object)]
pub struct ConnectionTokenInfo {
    pub session_token: String,
    pub master_token: String,
    pub session_token_expires_at_ms: Option<i64>,
    pub master_token_expires_at_ms: Option<i64>,
}

#[napi(object, object_to_js = false)]
pub struct ConnectionParams<'a> {
    pub options: HashMap<String, String>,
    pub session_parameters: HashMap<String, String>,
    pub open_external_browser_callback: Option<Function<'a, String, ()>>,
    pub token_info: Option<ConnectionTokenInfo>,
    pub no_connection_details: Option<bool>,
}

impl ConnectionParams<'_> {
    fn build_connection_options(&self) -> HashMap<String, Setting> {
        // TODO: temporary conversion, proper options mapping will be done later
        let mut core_options: HashMap<String, Setting> = self
            .options
            .iter()
            .map(|(k, v)| (k.clone(), Setting::String(v.clone())))
            .collect();

        if std::env::var(NODE_TLS_REJECT_UNAUTHORIZED).as_deref() == Ok("0") {
            core_options.insert(
                param_names::TLS_SKIP_VERIFY.as_str().to_string(),
                Setting::String("true".to_string()),
            );
        }
        if let Ok(ca_path) = std::env::var(NODE_EXTRA_CA_CERTS)
            && !ca_path.is_empty()
        {
            core_options.insert(
                param_names::EXTRA_ROOT_STORE_PATH.as_str().to_string(),
                Setting::String(ca_path),
            );
        }

        if let Some(token_info) = &self.token_info {
            core_options.insert(
                param_names::SESSION_TOKEN.as_str().to_string(),
                Setting::String(token_info.session_token.clone()),
            );
            core_options.insert(
                param_names::MASTER_TOKEN.as_str().to_string(),
                Setting::String(token_info.master_token.clone()),
            );
        }

        core_options
    }
}

#[napi]
pub struct Connection {
    session: Session,
}

#[napi]
impl Connection {
    #[napi(constructor)]
    pub fn new(env: &Env, params: ConnectionParams) -> Result<Self> {
        let database_handle = BRIDGE.driver.database_new();
        let conn_handle = BRIDGE.driver.connection_new();
        let session = Session::new(conn_handle, database_handle);

        let connection_options = params.build_connection_options();
        let browser_opener = params
            .open_external_browser_callback
            .map(browser_opener_from_js)
            .transpose()?;

        block_on(async {
            BRIDGE.driver.database_init(database_handle)?;
            BRIDGE
                .driver
                .connection_set_options(
                    conn_handle,
                    connection_options,
                    params.no_connection_details.unwrap_or(false),
                    None,
                )
                .await?;
            if !params.session_parameters.is_empty() {
                BRIDGE
                    .driver
                    .connection_set_session_parameters(conn_handle, params.session_parameters)
                    .await?;
            }
            if let Some(opener) = browser_opener {
                BRIDGE
                    .driver
                    .connection_set_browser_opener(conn_handle, opener)
                    .await?;
            }
            BRIDGE
                .driver
                .set_wrapper_identity(
                    conn_handle,
                    WrapperIdentity {
                        // TODO: pass this from nodejs (function arguments)
                        driver_name: "JavaScript".to_string(),
                        driver_version: "4.0.0-beta.0".to_string(),
                        language_runtime: "nodejs".to_string(),
                        language_version: "24.0.0".to_string(),
                        language_compiler: None,
                        release_type: None,
                        application_path: None,
                    },
                )
                .await?;

            // A connection built from session and master tokens never goes through `connect()`,
            // so it is initialized here. This is not cheap yet: it runs the full core connection
            // setup on the JS thread, and on macOS loading the system root certificates alone takes
            // about 100 ms. SNOW-4249447 and SNOW-4249371 track making it cheap.
            if params.token_info.is_some() {
                BRIDGE
                    .driver
                    .connection_init(None, conn_handle, database_handle)
                    .await?;
            }

            Ok::<_, ApiError>(())
        })
        .map_err(|e| e.to_js_error(*env))?;

        Ok(Self { session })
    }

    #[napi]
    pub fn connect(&self, env: &Env) -> Result<AsyncBlock<()>> {
        let session = self.session.clone();
        async_to_js(env, async move { session.connect().await })
    }

    #[napi]
    pub fn is_up(&self) -> bool {
        block_on(self.session.is_up())
    }

    #[napi]
    pub fn is_valid_async(&self, env: &Env) -> Result<AsyncBlock<bool>> {
        let session = self.session.clone();
        async_to_js(env, async move {
            Ok::<bool, BridgeError>(session.is_valid().await)
        })
    }

    #[napi]
    pub fn get_session_parameters(&self, env: &Env) -> Result<KnownSessionParameters> {
        block_on(self.session.known_session_parameters()).map_err(|e| e.to_js_error(*env))
    }

    #[napi]
    pub fn get_token_info(&self, env: &Env) -> Result<Option<ConnectionTokenInfo>> {
        let Some(info) = block_on(self.session.token_info()).map_err(|e| e.to_js_error(*env))?
        else {
            return Ok(None);
        };
        let (Some(session_token), Some(master_token)) = (info.session_token, info.master_token)
        else {
            return Ok(None);
        };
        Ok(Some(ConnectionTokenInfo {
            session_token: session_token.reveal().to_string(),
            master_token: master_token.reveal().to_string(),
            session_token_expires_at_ms: info.session_token_expires_at_ms,
            master_token_expires_at_ms: info.master_token_expires_at_ms,
        }))
    }

    #[napi]
    pub fn execute(&self, env: &Env, params: ExecuteParams) -> Result<Statement> {
        Statement::execute(self.session.clone(), params).map_err(|e| e.to_js_error(*env))
    }

    #[napi]
    pub fn get_query_status(&self, env: &Env, query_id: String) -> Result<AsyncBlock<QueryStatus>> {
        let session = self.session.clone();
        async_to_js(
            env,
            async move { query::get_status(&session, &query_id).await },
        )
    }

    #[napi]
    pub fn get_query_status_throw_if_error(
        &self,
        env: &Env,
        query_id: String,
    ) -> Result<AsyncBlock<QueryStatus>> {
        let session = self.session.clone();
        async_to_js(env, async move {
            query::get_status_throw_if_error(&session, &query_id).await
        })
    }

    #[napi]
    pub fn wait_for_query_result(
        &self,
        env: &Env,
        query_id: String,
        retry_interval_ms: Option<u32>,
    ) -> Result<AsyncBlock<()>> {
        let session = self.session.clone();
        let retry_interval = retry_interval_ms.map(|ms| Duration::from_millis(ms as u64));
        async_to_js(env, async move {
            query::wait_for_result(&session, query_id, retry_interval).await
        })
    }

    #[napi]
    pub fn get_query_result(&self, query_id: String) -> Statement {
        Statement::from_query_id(self.session.clone(), query_id)
    }

    #[napi]
    pub fn is_an_error(&self, status: QueryStatus) -> bool {
        status.is_an_error()
    }

    #[napi]
    pub fn is_still_running(&self, status: QueryStatus) -> bool {
        status.is_still_running()
    }

    // TODO: destroy does not release core handles when the connection was never
    // established.
    #[napi]
    pub fn destroy(&self, env: &Env) -> Result<AsyncBlock<()>> {
        let session = self.session.clone();
        async_to_js(env, async move { session.close().await })
    }
}

/// Adapts `openExternalBrowserCallback` to the synchronous opener core invokes.
fn browser_opener_from_js(callback: Function<String, ()>) -> Result<BrowserOpenFn> {
    let open_in_js = callback.build_threadsafe_function::<String>().build()?;
    Ok(Arc::new(move |url: &str| {
        let (outcome_tx, outcome_rx) = std::sync::mpsc::sync_channel(1);
        let queued = open_in_js.call_with_return_value(
            url.to_string(),
            ThreadsafeFunctionCallMode::NonBlocking,
            move |returned, _| {
                let _ = outcome_tx.send(returned.map_err(|thrown| thrown.reason));
                Ok(())
            },
        );
        if queued != Status::Ok {
            return Err(format!("openExternalBrowserCallback failed: {queued}"));
        }
        outcome_rx
            .recv()
            .unwrap_or_else(|_| Err("openExternalBrowserCallback did not return".into()))
    }))
}
