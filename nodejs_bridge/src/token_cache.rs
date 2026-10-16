use napi::bindgen_prelude::{
    FnArgs, FromNapiValue, Function, JsObjectValue, JsValuesTupleIntoVec, Object, Promise, block_on,
};
use napi::threadsafe_function::ThreadsafeFunction;
use napi_derive::napi;
use sf_core::token_cache::{
    CacheKey, KeyringTokenCache, TokenCache, TokenCacheError, TokenRemovalSnafu,
    TokenRetrievalSnafu, TokenStorageSnafu, build_cache_key,
};
use sf_core::utils::sync::RwLockRecoverExt;
use snafu::ResultExt;
use std::future::Future;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(30);

type JsCallback<Args, Ret> =
    ThreadsafeFunction<Args, Promise<Ret>, Args, napi::Status, false, true>;

struct CredentialManagerCallbacks {
    read: JsCallback<FnArgs<(String,)>, Option<String>>,
    write: JsCallback<FnArgs<(String, String)>, ()>,
    remove: JsCallback<FnArgs<(String,)>, ()>,
}

impl CredentialManagerCallbacks {
    fn read(&self, key: String) -> napi::Result<Option<String>> {
        wait_for_callback(async {
            self.read
                .call_async_catch(FnArgs::from((key,)))
                .await?
                .await
        })
    }

    fn write(&self, key: String, token: String) -> napi::Result<()> {
        wait_for_callback(async {
            self.write
                .call_async_catch(FnArgs::from((key, token)))
                .await?
                .await
        })
    }

    fn remove(&self, key: String) -> napi::Result<()> {
        wait_for_callback(async {
            self.remove
                .call_async_catch(FnArgs::from((key,)))
                .await?
                .await
        })
    }
}

fn wait_for_callback<T>(future: impl Future<Output = napi::Result<T>>) -> napi::Result<T> {
    block_on(async {
        match tokio::time::timeout(CALLBACK_TIMEOUT, future).await {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!(
                    timeout_secs = CALLBACK_TIMEOUT.as_secs(),
                    "Custom credential manager callback timed out"
                );
                Err(napi::Error::new(
                    napi::Status::Cancelled,
                    "Custom credential manager callback timed out",
                ))
            }
        }
    })
}

pub(crate) struct NodeTokenCache {
    callbacks: RwLock<Option<Arc<CredentialManagerCallbacks>>>,
    fallback: OnceLock<Arc<dyn TokenCache>>,
}

impl NodeTokenCache {
    pub(crate) fn new() -> Self {
        Self {
            callbacks: RwLock::new(None),
            fallback: OnceLock::new(),
        }
    }

    fn callbacks(&self) -> Option<Arc<CredentialManagerCallbacks>> {
        self.callbacks.read_recover().clone()
    }

    fn fallback(&self) -> Result<Arc<dyn TokenCache>, TokenCacheError> {
        if let Some(cache) = self.fallback.get() {
            return Ok(Arc::clone(cache));
        }
        let cache: Arc<dyn TokenCache> = Arc::new(KeyringTokenCache::new()?);
        Ok(Arc::clone(self.fallback.get_or_init(|| cache)))
    }

    fn set_callbacks(&self, callbacks: Option<CredentialManagerCallbacks>) {
        *self.callbacks.write_recover() = callbacks.map(Arc::new);
    }
}

impl TokenCache for NodeTokenCache {
    fn add_token(&self, key: &CacheKey, token_value: &str) -> Result<(), TokenCacheError> {
        let Some(callbacks) = self.callbacks() else {
            return self.fallback()?.add_token(key, token_value);
        };
        callbacks
            .write(build_cache_key(key), token_value.to_string())
            .boxed()
            .context(TokenStorageSnafu)
    }

    fn remove_token(&self, key: &CacheKey) -> Result<(), TokenCacheError> {
        let Some(callbacks) = self.callbacks() else {
            return self.fallback()?.remove_token(key);
        };
        callbacks
            .remove(build_cache_key(key))
            .boxed()
            .context(TokenRemovalSnafu)
    }

    fn get_token(&self, key: &CacheKey) -> Result<Option<String>, TokenCacheError> {
        let Some(callbacks) = self.callbacks() else {
            return self.fallback()?.get_token(key);
        };
        callbacks
            .read(build_cache_key(key))
            .boxed()
            .context(TokenRetrievalSnafu)
    }
}

#[napi(
    ts_args_type = "manager: { read: (key: string) => Promise<string | null | undefined>, write: (key: string, token: string) => Promise<void>, remove: (key: string) => Promise<void> } | null"
)]
pub fn set_custom_credential_manager(manager: Option<Object>) -> napi::Result<()> {
    let callbacks = manager
        .map(|manager| -> napi::Result<_> {
            Ok(CredentialManagerCallbacks {
                read: threadsafe(manager.get_named_property("read")?)?,
                write: threadsafe(manager.get_named_property("write")?)?,
                remove: threadsafe(manager.get_named_property("remove")?)?,
            })
        })
        .transpose()?;
    crate::BRIDGE.token_cache.set_callbacks(callbacks);
    Ok(())
}

fn threadsafe<Args: JsValuesTupleIntoVec + 'static, Ret: FromNapiValue + 'static>(
    function: Function<'_, Args, Promise<Ret>>,
) -> napi::Result<JsCallback<Args, Ret>> {
    function.build_threadsafe_function().weak::<true>().build()
}
