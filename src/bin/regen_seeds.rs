//! outer_source の各情報源のSeed（`data/seed/*.json`）を手動で再生成するツール。
//! main.rs（bot/api本体）とは独立に動く。プロジェクトルートで
//! `cargo run --bin regen_seeds` を実行し、差分を確認してcommit/pushする。
//! Heroku等では実行時のファイル書き込みが揮発するため、Seedはこのツールで
//! 生成してリポジトリに含めておく運用にしている。

use astesiabot_rust::engine::external_source::SEED_JOBS;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let mut had_failure = false;
    for job in SEED_JOBS {
        print!("[{}] {} を更新中... ", job.name, job.path);
        match (job.update)().await {
            Ok(()) => println!("OK"),
            Err(e) => {
                had_failure = true;
                println!("失敗: {e}");
            }
        }
    }

    if had_failure {
        eprintln!("一部のSeed更新に失敗しました。ネットワーク状況を確認して再実行してください。");
        std::process::exit(1);
    }

    println!("Seedの更新が完了しました。`git status` で差分を確認してcommit/pushしてください。");
}
