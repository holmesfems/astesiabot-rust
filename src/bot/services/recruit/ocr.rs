use crate::bot::data::Error;
use serde::Deserialize;
use std::sync::Mutex;

/// Vision APIのバージョン別エンドポイント
const ENDPOINTS: &[&str] = &[
    "https://vision.googleapis.com/v1/images:annotate",
    "https://vision.googleapis.com/v1p1beta1/images:annotate",
    "https://vision.googleapis.com/v1p2beta1/images:annotate",
    "https://vision.googleapis.com/v1p3beta1/images:annotate",
    "https://vision.googleapis.com/v1p4beta1/images:annotate",
];

/// OCR の結果。「文字が無い画像」と「Vision が使えない」を区別する
/// （前者は他の画像の処理を続けてよく、後者は広域障害の可能性があり打ち切る）。
#[derive(Debug, PartialEq, Eq)]
pub enum OcrText {
    Text(String),
    /// Vision は成功したが、画像に文字が無い
    NoText,
    /// 全エンドポイントが失敗した
    Unavailable,
}

impl From<Option<String>> for OcrText {
    fn from(text: Option<String>) -> Self {
        text.map_or(OcrText::NoText, OcrText::Text)
    }
}

/// 前回成功したエンドポイントのインデックスを記憶
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

/// Vision API 1回の待ち時間の上限。Vision は画像を Discord の CDN から取りに行くので、
/// 外部データ取得（7秒）より長めにする。無いと応答しないエンドポイントでその画像の処理が止まったままになる。
const VISION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// 応答の全文テキスト。テキストが無い（空も含む）なら None。
fn full_text(parsed: &VisionResponse) -> Option<String> {
    parsed
        .responses
        .first()
        .and_then(|r| r.text_annotations.first())
        .map(|t| t.description.clone())
        .filter(|t| !t.is_empty())
}

/// 1つのエンドポイントにリクエストを投げ、全文テキストを返す。
/// 応答が成功で画像に文字が無いなら Ok(None)。エンドポイント側の失敗（通信・HTTP エラー・
/// 応答が読めない）だけを Err にする。文字の無い画像はどのエンドポイントでも結果が同じなので、
/// 呼び出し側は Err のときだけ次のエンドポイントを試す（有料の呼び出しを無駄に増やさない）。
async fn try_endpoint(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    image_uri: &str,
) -> Result<Option<String>, Error> {
    let body = serde_json::json!({
        "requests": [{
            "image": { "source": { "imageUri": image_uri } },
            "features": [{ "type": "TEXT_DETECTION" }]
        }]
    });

    // APIキーはURLに載せずヘッダーで渡す（reqwestのエラー文言にURLがそのまま出るため）。
    let resp = client
        .post(endpoint)
        .header("X-goog-api-key", api_key)
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(format!("Vision API returned {}", resp.status()).into());
    }
    let parsed: VisionResponse = resp.json().await?;
    Ok(full_text(&parsed))
}

/// 応答が返るまでエンドポイントを順に試す。
/// 前回成功→ランダム順で残りを試す→成功を記憶。文字が無い画像も「成功」なので、そこで止める。
pub async fn get_text(image_uri: &str) -> Result<OcrText, Error> {
    let api_key =
        std::env::var("CLOUDVISION_API_KEY").map_err(|_| "CLOUDVISION_API_KEY not set")?;
    let client = reqwest::Client::builder().timeout(VISION_TIMEOUT).build()?;

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
            Ok(text) => {
                *LAST_OK.lock().unwrap() = Some(idx);
                return Ok(text.into());
            }
            Err(_) => continue,
        }
    }
    *LAST_OK.lock().unwrap() = None;
    Ok(OcrText::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> VisionResponse {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn ocr_text_distinguishes_no_text_from_text() {
        assert_eq!(OcrText::from(None), OcrText::NoText);
        assert_eq!(
            OcrText::from(Some("a".to_string())),
            OcrText::Text("a".to_string())
        );
    }

    #[test]
    fn full_text_takes_first_annotation() {
        let r = parse(
            r#"{"responses":[{"textAnnotations":[{"description":"上級エリート\n先鋒"},{"description":"上級エリート"}]}]}"#,
        );
        assert_eq!(full_text(&r).as_deref(), Some("上級エリート\n先鋒"));
    }

    #[test]
    fn full_text_is_none_for_image_without_text() {
        // 文字の無い画像では Vision は textAnnotations を返さない（成功応答）
        assert_eq!(full_text(&parse(r#"{"responses":[{}]}"#)), None);
        assert_eq!(full_text(&parse(r#"{"responses":[]}"#)), None);
        assert_eq!(
            full_text(&parse(
                r#"{"responses":[{"textAnnotations":[{"description":""}]}]}"#
            )),
            None
        );
    }
}
