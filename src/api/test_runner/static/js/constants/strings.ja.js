// Step 1 搬出: templates/tr_index.html の <script> 内に埋め込まれていた日本語UI文言。
// main.js からは T.<key> で参照する。連結で組み立てていた文字列（語順が言語で変わるもの）は
// 関数にしてある（詳細は tr_refactor_design.md 3.2）。
export const STRINGS = {
  // ---- parser (parseProcedure 内の表示用フォールバック文言) ----
  notSpecified: '（記載なし）',
  simpleFormatSectionTitle: '手順',

  // ---- 共通の「無題」フォールバック ----
  untitled: '（無題）',
  untitledBare: '無題',
  untitledProcedure: '（無題の手順）',
  untitledSection: '（無題の節）',

  // ---- 手順プレビュー（開始前の確認モーダルから開くモーダル） ----
  noPreambleOrGlossary: '（前置き・用語定義はありません）',
  docTitleLabel: '文書タイトル',
  sectionCountLabel: '節数',
  itemCountLabel: '項目数',
  readPreambleSummary: '前置き（概要・用語定義）を読む',
  itemCountBadge: function (n) { return n + '項目'; },
  sectionNumberTitle: function (n, title) { return '第' + n + '節: ' + title; },
  procedureLinkSummary: function (sections, items) { return '（' + sections + '節 / ' + items + '項目）'; },

  // ---- 配布物（materials） ----
  open: '開く',
  copyLink: 'リンクをコピー',
  providedFilesHeading: '配布物',
  providedFileHeading: '配布物',
  linkCopiedToast: 'リンクをコピーしました',
  noDescription: '（説明はありません）',

  // ---- ビルド入力 ----
  enterBuildPrompt: '実施ビルドを入力してください',
  buildLabel: 'ビルド',
  buildRequiredLabel: function (hint) { return '実施ビルド（必須' + (hint ? '・' + hint : '') + '）'; },

  // ---- 確認モーダルの開始ボタン ----
  continueLabel: '続ける',
  readyLabel: '準備完了！',

  // ---- 手順の読み取りエラー ----
  couldNotReadProcedure: '手順を読み取れませんでした',
  parseErrorHint:
    '<div style="margin-top:8px;font-size:12px;">Markdown の見出し（<code style="background:rgba(255,255,255,0.15);padding:1px 5px;border-radius:4px;">### 節タイトル</code>）と表（<code style="background:rgba(255,255,255,0.15);padding:1px 5px;border-radius:4px;">| 番号 | 手順 | 期待結果 |</code>）の形式、またはタブ／「→」区切りの簡易形式で記述してください。</div>',
  noItemsDetected: '項目を1つも検出できませんでした。ファイルの中身や形式をご確認ください。',
  noTextEntered: 'テキストが入力されていません。',
  fileReadError: function (msg) { return 'ファイルの読み込み中にエラーが発生しました: ' + msg; },

  // ---- スタート画面トースト ----
  aiPromptCopiedToast: 'コピーしました。AI に貼り付けて、続けて元の手順を貼ってください',
  sampleALoadedToast: 'サンプルAを貼り付け欄にセットしました。「🚀 開始する」で始められます',
  sampleBLoadedToast: 'サンプルBを貼り付け欄にセットしました。「🚀 開始する」で始められます',

  // ---- 進捗の書き出し / 読み込み ----
  noProgressToExport: '書き出せる進捗がありません',
  progressExported: function (fname) { return '進捗ファイルを書き出しました: ' + fname; },
  progressReadFailed: '進捗ファイルの読み込みに失敗しました',
  invalidProgressFormat: '進捗データの形式が正しくありません',
  progressProcedureParseFailed: '進捗データ内の手順を解析できませんでした',

  // ---- 進捗URL / 試験手順URLの共有 ----
  noProgressToShare: '共有できる進捗がありません',
  lzStringUnavailable: '圧縮ライブラリ(lz-string)が読み込めていないため、進捗URLを扱えません',
  resultUrlCopiedRawOnly: '結果URLをコピーしました（URLのみ）',
  resultUrlCopiedMarkdown: '結果URLをMarkdownリンクとしてコピーしました',
  noProcedureToShare: '共有できる試験手順がありません',
  procedureParseFailed: '試験手順を解析できませんでした',
  procedureUrlCopiedRawOnly: '試験手順のURLをコピーしました（URLのみ）',
  procedureUrlCopiedMarkdown: '試験手順のURLをMarkdownリンクとしてコピーしました',
  procedureTitleSuffix: ' 試験手順',
  urlProgressRestoreFailed: 'URLの進捗データを復元できませんでした（壊れているか、形式が古い可能性があります）',
  restoredCompletedFromUrl: '共有URLから完了済みの結果を復元しました',
  loadedSharedProcedure: '共有された試験手順を読み込みました',
  restoredProgressFromUrl: '共有URLから進捗を復元しました',
  resultLinkTitleText: function (title, answered, total, testerName) {
    var t = title + ' 結果 ' + answered + '/' + total;
    if (testerName) t += '（テスター: ' + testerName + '）';
    return t;
  },

  // ---- 日時ロケール ----
  dateLocale: 'ja-JP',

  // ---- 再開の案内（開始画面の resume-info） ----
  resumeInfoText: function (title, answered, total, testerName, when) {
    return '「' + title + '」 ' + answered + ' / ' + total + ' 項目 完了' +
      (testerName ? '　テスター: ' + testerName : '') +
      (when ? '　開始: ' + when : '');
  },

  // ---- ステップ画面 ----
  thisStepLabel: 'このステップ',
  percentComplete: function (pct) { return pct + '% 完了'; },
  remainingInSection: function (n) { return 'この節 残り ' + n + ' 項目'; },
  remainingOverall: function (items, sections) { return '全体 残り ' + items + ' 項目 / 残り ' + sections + ' 節'; },
  sectionOfTotal: function (n, total) { return '第' + n + '節 / 全' + total + '節'; },
  recordedLabel: function (label) { return '記録済み: ' + label; },
  itemNumberLabel: function (num) { return '番号 ' + num; },
  sectionNoteLabel: '節の注記',
  previousDuration: function (str) { return '（前回の所要時間 ' + str + '）'; },
  termHint: function (hasTerm, hasMaterial) {
    var what;
    if (hasTerm && hasMaterial) what = '金色の用語・水色の配布物';
    else if (hasTerm) what = '金色の用語';
    else what = '水色の配布物';
    return '✨ ' + what + 'をクリックすると説明が出ます';
  },

  // ---- NGの理由コメント ----
  ngCommentRequiredToast: 'NGの理由をコメントに入力してください',

  // ---- 用語 / 配布物ポップアップ ----
  termExplanationHeading: '用語解説',

  // ---- 節完了 / フィナーレ ----
  sectionCompleteHeading: function (n) { return '第' + n + '節 完了！'; },
  sectionTitleReveal: function (name) { return '称号: 「' + name + '」'; },
  toFinaleLabel: 'フィナーレへ ▶',
  nextSectionLabel: '次の節へ進む ▶',

  // ---- 業務連絡（buildBusinessReport） ----
  reportTitleWithBuild: function (title, buildLabel) {
    return buildLabel ? (title + '（ビルド ' + buildLabel + '）') : title;
  },
  reportGreeting: function (titleWithBuild) {
    return 'お疲れ様です。' + titleWithBuild + 'の試験について完了しましたので、ご確認をお願いします';
  },
  reportCounts: function (total, ok, ng) {
    return '試験項目: ' + total + '件　OK：　' + ok + '件　NG： ' + ng + '件';
  },
  reportNgListHeading: 'NG項目は以下の通りです：',
  reportNoCommentEntered: '（コメント未入力）',
  reportNgLine: function (num, comment) { return '・' + num + ' → ' + comment; },
  reportClosing: 'お手数をおかけしますが、よろしくお願いします',
  reportNoNgItems: 'NG項目はありません。',

  // ---- ランク ----
  rankFlawless: '完全無欠',
  rankS: 'ランク S',
  rankA: 'ランク A',
  rankB: 'ランク B',
  rankC: 'ランク C',

  // ---- 結果画面 ----
  statusNotDone: '未実施',
  notDoneOption: '（未実施）',
  notEntered: '（未入力）',
  testerLabel: 'テスター',
  dateTimeLabel: '実施日時',
  totalLabel: '合計',
  okRateLabel: 'OK率',
  overallRankLabel: '総合ランク',
  scoreLabel: '獲得スコア',
  maxComboLabel: '最大コンボ',
  totalTimeLabel: '総所要時間',

  // ---- エクスポート ----
  markdownTableHeader: '| 番号 | 節 | 手順 | 期待結果 | 結果 | コメント | 記録時刻 | 所要時間 |',
  buildPrefixLine: function (buildLabel) { return buildLabel ? ('ビルド: ' + buildLabel + '\n\n') : ''; },
  exportHeaderCellsBase: ['番号', '節', '手順', '期待結果', '結果', 'コメント', '記録時刻', '所要時間'],

  // ---- コピー / トースト共通 ----
  defaultCopiedMsg: 'コピーしました',
  copiedFallbackMsg: 'コピーしました（フォールバック）',
  copyFailedMsg: 'コピーに失敗しました。テキストを選択してコピーしてください。',
  reportCopiedToast: '業務連絡をコピーしました。チャットやメールに貼り付けてください',

  // ---- リスタート確認 ----
  confirmRestart: '本当に最初からやり直しますか？記録した結果はすべて消去されます。'
};
