use super::FetchError;
use serde_json::Value;
use std::time::Duration;

/// 各fetchで共通のタイムアウト・リトライ回数。
const FETCH_TIMEOUT: Duration = Duration::from_secs(7);
const FETCH_RETRIES: usize = 3;

/// 各情報源のfetchで共通利用する `reqwest::Client`（タイムアウト7s固定）。
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .expect("reqwest client should build with a fixed timeout")
}

/// URLからJSONをfetchする共通ヘルパー。最大 [`FETCH_RETRIES`] 回まで
/// リトライする（各回のタイムアウトは `client` 生成時に設定した7s）。
pub async fn fetch_json_with_retry(client: &reqwest::Client, url: &str) -> Result<Value, FetchError> {
    fetch_json_with_retry_headers(client, url, &[]).await
}

/// [`fetch_json_with_retry`] にヘッダー指定を足したもの。penguin-stats系の
/// fetchは `User-Agent: ArkPlanner` が必要なため、これを使う。
pub async fn fetch_json_with_retry_headers(
    client: &reqwest::Client,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<Value, FetchError> {
    let mut last_err: Option<FetchError> = None;
    for i in 0..FETCH_RETRIES {
        let mut req = client.get(url);
        print!("[fetch_json] try {i} url={url} headers={:?}\n", headers);
        for (key, value) in headers {
            req = req.header(*key, *value);
        }
        match req.send().await {
            Ok(resp) => match resp.error_for_status() {
                Ok(resp) => match resp.json::<Value>().await {
                    Ok(json) => return Ok(json),
                    Err(e) => last_err = Some(Box::new(e)),
                },
                Err(e) => last_err = Some(Box::new(e)),
            },
            Err(e) => last_err = Some(Box::new(e)),
        }
    }
    Err(last_err.expect("FETCH_RETRIES must be >= 1"))
}
