//! Downloading files from the server over HTTP (in the browser and on desktop).

use std::future::Future;

/// Downloads a file.
pub async fn fetch(url: String) -> Result<Vec<u8>, String> {
    start_fetch(url).await
}

/// Starts downloading a file straight away (not when the result is first
/// awaited, as [`fetch`] does), so several can be under way at once: start
/// them all, then await each.
pub fn start_fetch(url: String) -> impl Future<Output = Result<Vec<u8>, String>> {
    let (done, result) = async_channel::bounded(1);
    ehttp::fetch(ehttp::Request::get(&url), move |response| {
        let _ = done.try_send(response);
    });
    async move {
        match result.recv().await.map_err(|_| "the download was cancelled".to_string())? {
            Ok(response) if response.ok => Ok(response.bytes),
            Ok(response) => Err(format!("{url}: {} {}", response.status, response.status_text)),
            Err(error) => Err(format!("{url}: {error}")),
        }
    }
}
