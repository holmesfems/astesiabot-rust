// Step 1 搬出: templates/tr_index_en.html の <script> 内の演出用文言プール（褒め言葉・称号・NG引き止め等）。
// 内容・順序・要素数は一切変更していない（英語版そのまま）。

export const PRAISE_POOL = [
  'Well done', 'Nice touch', 'Smooth sailing, keep it up', 'Pretty slick',
  'Now that is good instinct', 'A superb judgment call', 'Beautifully done', 'Nearly flawless execution',
  'That precision deserves applause', 'A glimpse of real talent', 'The work of a true professional', 'Astonishing consistency',
  'This is what mastery looks like', 'Now this is pure artistry', 'A radiant, shining verdict', 'You can feel a legend beginning',
  'Something has awakened', 'The world is buzzing', 'This OK will go down in history', 'Shockwaves through the industry',
  'Truly divine', 'This is beyond human ability', 'Even the cosmos is blessing you', 'Precision that bends spacetime itself',
  'A guaranteed hall-of-famer', 'This will be passed down in legend', 'This is the pinnacle, this is truth', 'You are already a living legend',
  'The entire universe is applauding', 'The goddess of quality has smiled upon you', 'History was rewritten in this very moment', 'Words fail to describe this any longer',
  'The ultimate, beyond ultimate', 'This alone proves omniscience'
];

export const TESTER_NAME_POOL = [
  'Test Champion', 'Ultimate Tester', 'Bug Buster', 'Quality Guardian', 'Verification Master', 'Defect Hunter'
];

export const SECTION_TITLE_POOL = [
  'Galactic Test Commander', 'Ruler of the Taskbar', 'Guardian Deity of Quality', 'Sage of Verification', 'Master of the Click',
  'Rising Star of Verification', 'Chosen One of the OK Button', 'Apostle of the Procedure Document', 'Watcher Over a Bug-Free World', 'Hero of the Test Chamber',
  'Unbeatable Verification Master', 'Legendary Tester', 'Embodiment of Quality Assurance', 'Master of Window Management', 'Conqueror of Dialogs',
  'Living Embodiment of Perfectionism', 'King of the Verification Kingdom', 'Marvel Who Needs No Debugging', 'Pinnacle of QA', 'Vanquisher of Regressions',
  'The Procedure-Execution Machine', 'Creator of the Quality Cosmos', 'Champion of Test Cases'
];

export const SECTION_PRAISE_POOL = [
  'This section was, frankly, perfect.',
  'Your fingertips are weaving quality itself.',
  'Everyone involved is applauding (by our own internal metrics).',
  'At this rate, bugs will retreat on their own.',
  'Let us dash through the next section without blinking.',
  'This record has been quietly, but surely, etched into history.',
  'Another legend has just been born in the world of testing.',
  'Save the celebration for after the victory — no toasting quite yet.',
  'Completing this section is, in fact, a cosmic-scale event.',
  'The procedure document is grateful to you (probably).'
];

export const FINALE_PRAISE_POOL = [
  'A grand round of applause for racing through every section.',
  'This test runner may well have existed just for you.',
  'The results table is waiting for you. Go look at it with pride.',
  'Today has been etched into the history of quality.',
  'Great work. See you again for the next test.'
];

export const TIME_PRAISE_FAST = [
  'A lightning-fast verdict.',
  'A verdict at the speed of light — there was no time to blink.',
  'Your fingers moved too fast to see.',
  'A tempo that looked like pure reflex.',
  'A judgment sharp enough to shave off tenths of a second.',
  'It was all over in the blink of an eye.',
  'A pace as smooth as the wind.'
];

export const TIME_PRAISE_STEADY = [
  'Proceeding at an ideal tempo.',
  'A rhythm you could feel, like a true craftsperson.',
  'A stable, comfortable pace throughout.',
  'An unforced, genuinely easy-to-follow tempo.',
  'That steady, unwavering rhythm is impressive.',
  'A pace that radiates steadiness.'
];

export const TIME_PRAISE_CAREFUL = [
  'Care is quality itself — respect for that attitude.',
  'A gaze that let nothing slip past unnoticed.',
  'The courage to take your time is a skill in itself.',
  'That unhurried, thorough attitude inspires real confidence.',
  'That carefulness is quietly upholding the quality here.',
  'You have the strength to check things through without rushing.'
];

export const NG_DETERRENT_POOL = [
  'Have you really read the expected result again, carefully?',
  'Are you confident you did not skip a single step?',
  'Could that be a simple misreading? Please take one more careful look.',
  'Could it just be hidden behind a dialog or another window?',
  'Recording an NG requires a concrete comment. Are you ready to write one?',
  'You can still turn back right now.',
  'We trust your eye for detail — but just in case, take one more look.',
  'A streak of OKs grows your combo. But a false OK is never acceptable.',
  'Take a breath and compare the expected result against the actual screen once more, will you?',
  'Are you confident you would stand by this call even later?',
  'Did you click in a hurry? Take a moment, and try again.',
  'If it really is different, mark it NG without hesitation — that is what protects quality.',
  'The step numbers, conditions, and order — did you follow the procedure exactly?',
  'There is still time to redo this. No need to rush, check once more.',
  'If you are certain it is different, catching it here is exactly the job — proceed to NG without hesitation.',
  'Can you reproduce that behavior by trying the operation again?',
  'An NG is a heavy record. That is exactly why you should be certain before proceeding.',
  'What you are seeing and what is expected — are they really not matching?'
];

export const NG_DODGE_PHRASES = ['Whoa there', 'Really?', 'Careful now', 'Check the expected result once more…', 'Not yet?', 'Look closely?'];


export const PHRASES = {
  PRAISE_POOL,
  TESTER_NAME_POOL,
  SECTION_TITLE_POOL,
  SECTION_PRAISE_POOL,
  FINALE_PRAISE_POOL,
  TIME_PRAISE_FAST,
  TIME_PRAISE_STEADY,
  TIME_PRAISE_CAREFUL,
  NG_DETERRENT_POOL,
  NG_DODGE_PHRASES,
};
