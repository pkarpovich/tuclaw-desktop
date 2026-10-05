use std::future::Future;
use std::sync::OnceLock;

use futures::FutureExt;
use futures::future::BoxFuture;
use tokio::runtime::{Builder, Handle, Runtime};
use tokio::task::AbortHandle;

use super::transport::ApiError;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

pub(crate) fn handle() -> Handle {
    let runtime = RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("tuclaw-core-io")
            .enable_all()
            .build()
            .expect("the core I/O runtime starts")
    });
    runtime.handle().clone()
}

struct AbortOnDrop(AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(crate) fn spawn<F, T>(handle: &Handle, future: F) -> BoxFuture<'static, Result<T, ApiError>>
where
    F: Future<Output = Result<T, ApiError>> + Send + 'static,
    T: Send + 'static,
{
    let task = handle.spawn(future);
    let guard = AbortOnDrop(task.abort_handle());
    async move {
        let finished = task.await;
        drop(guard);
        match finished {
            Ok(result) => result,
            Err(error) => Err(ApiError::Transport(error.to_string())),
        }
    }
    .boxed()
}
