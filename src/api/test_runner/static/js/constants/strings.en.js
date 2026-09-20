// Step 1 搬出: templates/tr_index_en.html の <script> 内に埋め込まれていた英語UI文言。
// キー集合は strings.ja.js と完全一致させること（不変条件C）。
export const STRINGS = {
  // ---- parser (parseProcedure 内の表示用フォールバック文言) ----
  notSpecified: '(not specified)',
  simpleFormatSectionTitle: 'Steps',

  // ---- 共通の「無題」フォールバック ----
  untitled: '(Untitled)',
  untitledBare: 'Untitled',
  untitledProcedure: '(Untitled procedure)',
  untitledSection: '(Untitled section)',

  // ---- 手順プレビュー（開始前の確認モーダルから開くモーダル） ----
  noPreambleOrGlossary: '(No preamble or glossary)',
  docTitleLabel: 'Document title',
  sectionCountLabel: 'Sections',
  itemCountLabel: 'Items',
  readPreambleSummary: 'Read the preamble (overview &amp; glossary)',
  itemCountBadge: function (n) { return n + ' items'; },
  sectionNumberTitle: function (n, title) { return 'Section ' + n + ': ' + title; },
  procedureLinkSummary: function (sections, items) { return ' (' + sections + ' sections / ' + items + ' items)'; },

  // ---- 配布物（materials） ----
  open: 'Open',
  copyLink: 'Copy link',
  providedFilesHeading: 'Provided files',
  providedFileHeading: 'Provided file',
  linkCopiedToast: 'Link copied',
  noDescription: '(No description)',

  // ---- ビルド入力 ----
  enterBuildPrompt: 'Please enter the build under test',
  buildLabel: 'Build',
  buildRequiredLabel: function (hint) { return 'Build under test (required' + (hint ? ', ' + hint : '') + ')'; },

  // ---- 確認モーダルの開始ボタン ----
  continueLabel: 'Continue',
  readyLabel: 'Ready!',

  // ---- 手順の読み取りエラー ----
  couldNotReadProcedure: 'Could not read the procedure',
  parseErrorHint:
    '<div style="margin-top:8px;font-size:12px;">Please use a Markdown heading (<code style="background:rgba(255,255,255,0.15);padding:1px 5px;border-radius:4px;">### Section title</code>) with a table (<code style="background:rgba(255,255,255,0.15);padding:1px 5px;border-radius:4px;">| No. | Step | Expected result |</code>), or the simple tab/"→"-separated format.</div>',
  noItemsDetected: 'No items could be detected. Please check the file contents and format.',
  noTextEntered: 'No text has been entered.',
  fileReadError: function (msg) { return 'An error occurred while reading the file: ' + msg; },

  // ---- スタート画面トースト ----
  aiPromptCopiedToast: 'Copied. Paste it into the AI, then paste your original procedure after it',
  sampleALoadedToast: 'Sample A loaded into the paste box. Press "🚀 Start" to begin',
  sampleBLoadedToast: 'Sample B loaded into the paste box. Press "🚀 Start" to begin',

  // ---- 進捗の書き出し / 読み込み ----
  noProgressToExport: 'There is no progress to export',
  progressExported: function (fname) { return 'Progress file exported: ' + fname; },
  progressReadFailed: 'Failed to read the progress file',
  invalidProgressFormat: 'The progress data format is invalid',
  progressProcedureParseFailed: 'Could not parse the procedure inside the progress data',

  // ---- 進捗URL / 試験手順URLの共有 ----
  noProgressToShare: 'There is no progress to share',
  lzStringUnavailable: 'The compression library (lz-string) failed to load, so progress URLs are unavailable',
  resultUrlCopiedRawOnly: 'Result URL copied (URL only)',
  resultUrlCopiedMarkdown: 'Result URL copied as a Markdown link',
  noProcedureToShare: 'There is no procedure to share',
  procedureParseFailed: 'Could not parse the procedure',
  procedureUrlCopiedRawOnly: 'Procedure URL copied (URL only)',
  procedureUrlCopiedMarkdown: 'Procedure URL copied as a Markdown link',
  procedureTitleSuffix: ' test procedure',
  urlProgressRestoreFailed: 'Could not restore progress from the URL (it may be corrupted or in an old format)',
  restoredCompletedFromUrl: 'Restored completed results from the shared URL',
  loadedSharedProcedure: 'Loaded the shared procedure',
  restoredProgressFromUrl: 'Progress restored from the shared URL',
  resultLinkTitleText: function (title, answered, total, testerName) {
    var t = title + ' result ' + answered + '/' + total;
    if (testerName) t += ' (tester: ' + testerName + ')';
    return t;
  },

  // ---- 日時ロケール ----
  dateLocale: 'en-US',

  // ---- 再開の案内（開始画面の resume-info） ----
  resumeInfoText: function (title, answered, total, testerName, when) {
    return '"' + title + '" ' + answered + ' / ' + total + ' items complete' +
      (testerName ? '  Tester: ' + testerName : '') +
      (when ? '  Started: ' + when : '');
  },

  // ---- ステップ画面 ----
  thisStepLabel: 'This step',
  percentComplete: function (pct) { return pct + '% complete'; },
  remainingInSection: function (n) { return n + ' items left in this section'; },
  remainingOverall: function (items, sections) { return items + ' items / ' + sections + ' sections left overall'; },
  sectionOfTotal: function (n, total) { return 'Section ' + n + ' / ' + total; },
  recordedLabel: function (label) { return 'Recorded: ' + label; },
  itemNumberLabel: function (num) { return 'No. ' + num; },
  sectionNoteLabel: 'Section note',
  previousDuration: function (str) { return '(Previous duration ' + str + ')'; },
  termHint: function (hasTerm, hasMaterial) {
    var what;
    if (hasTerm && hasMaterial) what = 'a gold term or a light-blue file';
    else if (hasTerm) what = 'a gold term';
    else what = 'a light-blue file';
    return '✨ Click ' + what + ' to see its explanation';
  },

  // ---- NGの理由コメント ----
  ngCommentRequiredToast: 'Please enter a comment explaining the NG',

  // ---- 用語 / 配布物ポップアップ ----
  termExplanationHeading: 'Term explanation',

  // ---- 節完了 / フィナーレ ----
  sectionCompleteHeading: function (n) { return 'Section ' + n + ' complete!'; },
  sectionTitleReveal: function (name) { return 'Title: "' + name + '"'; },
  toFinaleLabel: 'To the finale ▶',
  nextSectionLabel: 'Next section ▶',

  // ---- 業務連絡（buildBusinessReport） ----
  reportTitleWithBuild: function (title, buildLabel) {
    return buildLabel ? ('"' + title + '" (build ' + buildLabel + ')') : ('"' + title + '"');
  },
  reportGreeting: function (titleWithBuild) {
    return 'Hi, testing for ' + titleWithBuild + ' is complete — please review the results below.';
  },
  reportCounts: function (total, ok, ng) {
    return 'Items tested: ' + total + '   OK: ' + ok + '   NG: ' + ng;
  },
  reportNgListHeading: 'The NG items are as follows:',
  reportNoCommentEntered: '(No comment entered)',
  reportNgLine: function (num, comment) { return '- ' + num + ' -> ' + comment; },
  reportClosing: 'Sorry for the extra work, and thanks in advance.',
  reportNoNgItems: 'There are no NG items.',

  // ---- ランク ----
  rankFlawless: 'Flawless',
  rankS: 'Rank S',
  rankA: 'Rank A',
  rankB: 'Rank B',
  rankC: 'Rank C',

  // ---- 結果画面 ----
  statusNotDone: 'Not done',
  notDoneOption: '(Not done)',
  notEntered: '(Not entered)',
  testerLabel: 'Tester',
  dateTimeLabel: 'Date/time',
  totalLabel: 'Total',
  okRateLabel: 'OK rate',
  overallRankLabel: 'Overall rank',
  scoreLabel: 'Score',
  maxComboLabel: 'Max combo',
  totalTimeLabel: 'Total time',

  // ---- エクスポート ----
  markdownTableHeader: '| No. | Section | Step | Expected result | Result | Comment | Recorded at | Duration |',
  buildPrefixLine: function (buildLabel) { return buildLabel ? ('Build: ' + buildLabel + '\n\n') : ''; },
  exportHeaderCellsBase: ['No.', 'Section', 'Step', 'Expected result', 'Result', 'Comment', 'Recorded at', 'Duration'],

  // ---- コピー / トースト共通 ----
  defaultCopiedMsg: 'Copied',
  copiedFallbackMsg: 'Copied (fallback)',
  copyFailedMsg: 'Copy failed. Please select the text and copy it manually.',
  reportCopiedToast: 'Report copied. Paste it into chat or email',

  // ---- リスタート確認 ----
  confirmRestart: 'Are you sure you want to start over? All recorded results will be cleared.'
};
