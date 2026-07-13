use crate::bot::data::Error;
use serde::Deserialize;
use std::sync::Mutex;

/// Vision APIのバージョン別エンドポイント（Python の _clientTypes 相当）
const ENDPOINTS: &[&str] = &[
    "https://vision.googleapis.com/v1/images:annotate",
    "https://vision.googleapis.com/v1p1beta1/images:annotate",
    "https://vision.googleapis.com/v1p2beta1/images:annotate",
    "https://vision.googleapis.com/v1p3beta1/images:annotate",
    "https://vision.googleapis.com/v1p4beta1/images:annotate",
];

/// 前回成功したエンドポイントのインデックスを記憶
/// （Python の _lastAvailabledClient 相当）
static LAST_OK: Mutex<Option<usize>> = Mutex::new(None);

#[derive(Deserialize)]
struct VisionResponse {
    responses: Vec<AnnotateResponse>,
}
#[derive(Deserialize)]
struct AnnotateResponse {
    #[serde(rename = "textAnnotations", default)]
    text_annotations: Vec<TextAnnotation>,
}
#[derive(Deserialize)]
struct TextAnnotation {
    description: String,
}

/// 1つのエンドポイントにリクエストを投げ、全文テキストを返す。
/// テキストが無ければ None。
async fn try_endpoint(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    image_uri: &str,
) -> Result<Option<String>, Error> {
    let url = format!("{endpoint}?key={api_key}");
    let body = serde_json::json!({
        "requests": [{
            "image": { "source": { "imageUri": image_uri } },
            "features": [{ "type": "TEXT_DETECTION" }]
        }]
    });

    let resp = client.post(&url).json(&body).send().await?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let parsed: VisionResponse = resp.json().await?;
    let text = parsed
        .responses
        .first()
        .and_then(|r| r.text_annotations.first())
        .map(|t| t.description.clone());
    Ok(text)
}

/// 全エンドポイントを Python と同じ戦略で試す。
/// 前回成功→ランダム順で残りを試す→成功を記憶。
pub async fn get_text(image_uri: &str) -> Result<Option<String>, Error> {
    let api_key =
        std::env::var("CLOUDVISION_API_KEY").map_err(|_| "CLOUDVISION_API_KEY not set")?;
    let client = reqwest::Client::new();

    // 試す順番を作る：前回成功したものを先頭に、残りを軽くシャッフル
    let mut order: Vec<usize> = (0..ENDPOINTS.len()).collect();
    {
        let last = *LAST_OK.lock().unwrap();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as usize;
        for i in (1..order.len()).rev() {
            let j = (seed.wrapping_mul(i + 1)) % (i + 1);
            order.swap(i, j);
        }
        if let Some(idx) = last {
            if let Some(pos) = order.iter().position(|&x| x == idx) {
                order.swap(0, pos);
            }
        }
    }

    for idx in order {
        match try_endpoint(&client, ENDPOINTS[idx], &api_key, image_uri).await {
            Ok(Some(text)) if !text.is_empty() => {
                *LAST_OK.lock().unwrap() = Some(idx);
                return Ok(Some(text));
            }
            _ => continue,
        }
    }
    *LAST_OK.lock().unwrap() = None;
    Ok(None)
}
