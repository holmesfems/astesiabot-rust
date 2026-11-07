//! skill の配布（`GET /api/risei/skill.zip`）。

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;


pub const SKILL_ZIP_PATH: &str = "/api/risei/skill.zip";

/// zip内のフォルダ名(=skill名)。展開してそのまま`.claude/skills/`に置ける形にする。
const SKILL_NAME: &str = "arknights-risei-api";

pub(super) const SKILL_MD_PATH: &str = "src/api/risei_skill/SKILL.md";

/// skill配布(`GET /api/risei/skill.zip`)だけのルーター。状態に依存しないので
/// `web_ui_router()`に載せる。`/api/risei`の他のルート([`router`])とはパスが重ならない
/// (静的パスはnestのワイルドカードより優先される)。
pub fn skill_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route(SKILL_ZIP_PATH, get(skill_zip))
}

/// skillのzipをリクエストごとにディスクから組み立てる。
/// SKILL.mdを直しても再ビルド不要で、配布物が古くなることも無い。
fn build_skill_zip() -> std::io::Result<Vec<u8>> {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let data = std::fs::read(SKILL_MD_PATH)?;
    writer
        .start_file(format!("{SKILL_NAME}/SKILL.md"), options)
        .map_err(std::io::Error::other)?;
    writer.write_all(&data)?;
    Ok(writer.finish().map_err(std::io::Error::other)?.into_inner())
}

async fn skill_zip() -> Response {
    match build_skill_zip() {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/zip".to_string()),
                (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{SKILL_NAME}.zip\"")),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => {
            // デプロイ事故（ファイル欠落等）を黙って隠さない。
            eprintln!("risei skill.zip の組み立てに失敗しました: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to build skill.zip").into_response()
        }
    }
}
