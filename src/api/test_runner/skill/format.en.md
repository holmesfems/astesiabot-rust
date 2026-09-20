You are formatting a test procedure document. Reformat the test items pasted below into the exact Markdown format specified. Do not add, remove, or reinterpret any content — only adjust the formatting. Do not fill in unclear points by guessing; leave them as-is.

## Output format

IMPORTANT: the structural markers below (`## `, `### `, the `Glossary:` line, the optional `Build:` and `Attachments:` lines, the table column header row, the square-bracket tag at the end of a section heading, and the `Note: ` note prefix) are what this app parses. Reproduce them exactly as shown.

Make line 1 `## <test title>`.

Next, add a `### Test overview` section with free text describing what is being tested and what needs to be prepared. If there are any terms to define, put a line reading exactly `Glossary:` below that, followed by a bulleted list in the form `* **term** … description` (the separator is the single ellipsis character "…", not three periods). If there are no terms, you may omit the `Glossary:` line entirely.

If the build under test is already decided, add a single line such as `Build: 1.2.3-beta4` to the preamble (the value itself can be anything). If instead the tester should look the build number up and type it in, start the value with `Enter` and put where to find the number in parentheses, like `Build: Enter (the number shown under "?" → "About" at the top right)`. If neither applies, omit the `Build:` line entirely.

If there are files handed to the tester before testing (sample data, assets, and so on), add a line reading exactly `Attachments:` to the preamble, followed by a bulleted list in the form `* **file name** … URL description` (the separator is the same ellipsis "…" used for the glossary). Write each description so it makes sense on its own, without referring to any other item. If there are no such files, omit the `Attachments:` line entirely.

Write each section (a group of test items) as `### 1. Section title [Windows]`, i.e. "number. title [target OS or environment]". The square-bracket tag must sit at the very end of the heading line, and it is optional — leave it off when there is nothing to tag.

Immediately below each section, place a table in this exact format:

| No. | Step | Expected result |
|---|---|---|
| (sequence number) | (step) | (expected result) |

Numbers must run continuously across the whole document (do not reset when a new section starts).

If there is a note after a table, write it as a line starting with `Note: `.

## Example

```markdown
## "Calcy" basic arithmetic test procedure (example)

### Test overview

Areas covered: the four arithmetic operations

Build: 1.0.3

Glossary:

* **Calcy** … a fictional calculator app (for illustration only)

### 1. Four arithmetic operations [Common]

| No. | Step | Expected result |
|---|---|---|
| 1 | Enter `1 + 1 =` | The display shows `2` |
| 2 | Enter `2 - 1 =` | The display shows `1` |

Note: This is just a formatting example.
```

## Writing rules (important)

- Write each row so it can be carried out from that row alone. References to other rows, such as "redo N", "same as above", "same as the previous item", or "as above", are forbidden. Rewrite every required action in full each time, without abbreviating.
- Write expected results as definite statements, specifying exactly what should be visible — values, positions, counts, and so on. Do not use words that require subjective judgment, such as "appropriately", "correctly", "reasonably", "roughly", "as before", or "without issue". If the original text uses such a word and it cannot be made concrete without changing its meaning, keep the word as-is and append `(Needs review: judgment criteria unclear)` at the end of the line.
- Wrap words you want bolded in `**word**`. Wrap key names or commands in backticks, like `word`.
- Do not use `|` inside table cells — replace it with the full-width character `｜`. Do not put line breaks inside a cell.

## How to respond

Return only the formatted Markdown, in a single code block. No preamble or commentary needed.

Here are the original test items: