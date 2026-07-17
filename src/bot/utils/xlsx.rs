//! riseiXXX系コマンドの `csv_file` オプション用xlsx出力（Python `stagesToExcelFile`/
//! `listToCSV`相当）。Discordの添付ファイルとして使う以外の用途が無いため`engine`ではなく
//! ここに置く。値の中身さえPython版と合っていればよく、フォント等の見た目は踏襲しない。
//!
//! Python版の参照ブロック(`Calculator.toDataFrame`)は換算行列・定番周回マップ行列まで
//! 含む連立方程式の生データだが、検証用途で実用上参照されないため、ここでは
//! 「理性価値」「基準ステージ」の2行だけに簡略化している。

use rust_xlsxwriter::{Workbook, XlsxError};
use std::collections::HashMap;

/// riseimaterials/riseistages/riseievents共通のステージ1行分。
pub struct StageExportRow {
    pub name: String,
    /// `columns`と同じ並び順のドロップ率(理性消費のLMDボーナス加算済み)。
    pub drop_values: Vec<f64>,
    pub ap_cost: f64,
}

/// ステージ検索系(riseimaterials/riseistages/riseievents)のxlsxを1シート分組み立てる。
///
/// - 1行目: 空セル + `columns`(アイテム名) + "理性消費"
/// - 2行目: "理性価値" + `value_row`
/// - 3行目: "基準ステージ" + `base_stage_row`(カテゴリの代表素材列のみ埋まる。他は空欄)
/// - 4行目以降: `stages`の1件ずつ
pub fn build_stage_export_xlsx(
    columns: &[&str],
    value_row: &[f64],
    base_stage_row: &[Option<String>],
    stages: &[StageExportRow],
) -> Result<Vec<u8>, XlsxError> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    let ap_cost_col = (columns.len() + 1) as u16;

    worksheet.write(0, 0, "")?;
    for (i, name) in columns.iter().enumerate() {
        worksheet.write(0, (i + 1) as u16, *name)?;
    }
    worksheet.write(0, ap_cost_col, "理性消費")?;

    worksheet.write(1, 0, "理性価値")?;
    for (i, v) in value_row.iter().enumerate() {
        worksheet.write(1, (i + 1) as u16, *v)?;
    }

    worksheet.write(2, 0, "基準ステージ")?;
    for (i, name) in base_stage_row.iter().enumerate() {
        if let Some(name) = name {
            worksheet.write(2, (i + 1) as u16, name.as_str())?;
        }
    }

    for (r, stage) in stages.iter().enumerate() {
        let row = (r + 3) as u32;
        worksheet.write(row, 0, stage.name.as_str())?;
        for (i, v) in stage.drop_values.iter().enumerate() {
            worksheet.write(row, (i + 1) as u16, *v)?;
        }
        worksheet.write(row, ap_cost_col, stage.ap_cost)?;
    }

    workbook.save_to_buffer()
}

/// riseikakinの1パック分。
pub struct KakinExportPack {
    pub name: String,
    /// 素材名(JA)→個数。
    pub contents: HashMap<String, f64>,
    /// `target_columns`と同じ並び順の集計値(総合効率/ガチャ効率/パック値段/合計理性価値/
    /// 純正源石換算/マネー換算/ガチャ数)。
    pub target_values: Vec<f64>,
}

/// riseikakinのxlsxを1シート分組み立てる（Python `listToCSV`相当）。
///
/// - 1行目: 空セル + `material_columns` + `target_columns`
/// - 2行目以降: `packs`の1件ずつ(素材列は個数、集計列は`target_values`)
/// - 最終行: "理性価値" + 各素材の理性価値(`value_row`) + 集計列は空欄
pub fn build_kakin_export_xlsx(
    material_columns: &[String],
    target_columns: &[&str],
    packs: &[KakinExportPack],
    value_row: &[f64],
) -> Result<Vec<u8>, XlsxError> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    let target_col_start = (material_columns.len() + 1) as u16;

    worksheet.write(0, 0, "")?;
    for (i, name) in material_columns.iter().enumerate() {
        worksheet.write(0, (i + 1) as u16, name.as_str())?;
    }
    for (i, name) in target_columns.iter().enumerate() {
        worksheet.write(0, target_col_start + i as u16, *name)?;
    }

    for (r, pack) in packs.iter().enumerate() {
        let row = (r + 1) as u32;
        worksheet.write(row, 0, pack.name.as_str())?;
        for (i, name) in material_columns.iter().enumerate() {
            let count = pack.contents.get(name).copied().unwrap_or(0.0);
            worksheet.write(row, (i + 1) as u16, count)?;
        }
        for (i, v) in pack.target_values.iter().enumerate() {
            worksheet.write(row, target_col_start + i as u16, *v)?;
        }
    }

    let value_row_idx = (packs.len() + 1) as u32;
    worksheet.write(value_row_idx, 0, "理性価値")?;
    for (i, v) in value_row.iter().enumerate() {
        worksheet.write(value_row_idx, (i + 1) as u16, *v)?;
    }

    workbook.save_to_buffer()
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{open_workbook_from_rs, Data, DataType, Reader, Xlsx};
    use std::io::Cursor;

    fn read_back(bytes: Vec<u8>) -> calamine::Range<Data> {
        let cursor = Cursor::new(bytes);
        let mut workbook: Xlsx<_> = open_workbook_from_rs(cursor).expect("valid xlsx");
        let name = workbook.sheet_names()[0].clone();
        workbook.worksheet_range(&name).expect("sheet exists")
    }

    #[test]
    fn stage_export_lays_out_header_reference_and_stage_rows() {
        let columns = ["源岩", "龙门币1000"];
        let value_row = [10.0, 1.0];
        let base_stage_row = [Some("1-7".to_string()), None];
        let stages = vec![
            StageExportRow {
                name: "1-7".to_string(),
                drop_values: vec![1.5, 0.5],
                ap_cost: 21.0,
            },
            StageExportRow {
                name: "S4-9".to_string(),
                drop_values: vec![2.0, 0.0],
                ap_cost: 18.0,
            },
        ];

        let bytes = build_stage_export_xlsx(&columns, &value_row, &base_stage_row, &stages).expect("build xlsx");
        let range = read_back(bytes);

        assert_eq!(range.get_value((0, 1)).unwrap().to_string(), "源岩");
        assert_eq!(range.get_value((0, 2)).unwrap().to_string(), "龙门币1000");
        assert_eq!(range.get_value((0, 3)).unwrap().to_string(), "理性消費");

        assert_eq!(range.get_value((1, 0)).unwrap().to_string(), "理性価値");
        assert_eq!(range.get_value((1, 1)).unwrap().as_f64(), Some(10.0));
        assert_eq!(range.get_value((1, 2)).unwrap().as_f64(), Some(1.0));

        assert_eq!(range.get_value((2, 0)).unwrap().to_string(), "基準ステージ");
        assert_eq!(range.get_value((2, 1)).unwrap().to_string(), "1-7");
        assert!(range.get_value((2, 2)).is_none_or(|c| matches!(c, Data::Empty)));

        assert_eq!(range.get_value((3, 0)).unwrap().to_string(), "1-7");
        assert_eq!(range.get_value((3, 1)).unwrap().as_f64(), Some(1.5));
        assert_eq!(range.get_value((3, 2)).unwrap().as_f64(), Some(0.5));
        assert_eq!(range.get_value((3, 3)).unwrap().as_f64(), Some(21.0));

        assert_eq!(range.get_value((4, 0)).unwrap().to_string(), "S4-9");
        assert_eq!(range.get_value((4, 3)).unwrap().as_f64(), Some(18.0));
    }

    #[test]
    fn kakin_export_fills_missing_materials_with_zero_and_blanks_value_row_targets() {
        let material_columns = vec!["源岩".to_string(), "赤金".to_string()];
        let target_columns = ["総合効率", "パック値段"];
        let mut pack1_contents = HashMap::new();
        pack1_contents.insert("源岩".to_string(), 10.0);
        let mut pack2_contents = HashMap::new();
        pack2_contents.insert("赤金".to_string(), 3.0);
        let packs = vec![
            KakinExportPack {
                name: "パックA".to_string(),
                contents: pack1_contents,
                target_values: vec![1.2, 1000.0],
            },
            KakinExportPack {
                name: "パックB".to_string(),
                contents: pack2_contents,
                target_values: vec![0.8, 2000.0],
            },
        ];
        let value_row = [0.05, 0.2];

        let bytes = build_kakin_export_xlsx(&material_columns, &target_columns, &packs, &value_row).expect("build xlsx");
        let range = read_back(bytes);

        assert_eq!(range.get_value((0, 1)).unwrap().to_string(), "源岩");
        assert_eq!(range.get_value((0, 2)).unwrap().to_string(), "赤金");
        assert_eq!(range.get_value((0, 3)).unwrap().to_string(), "総合効率");
        assert_eq!(range.get_value((0, 4)).unwrap().to_string(), "パック値段");

        assert_eq!(range.get_value((1, 0)).unwrap().to_string(), "パックA");
        assert_eq!(range.get_value((1, 1)).unwrap().as_f64(), Some(10.0));
        assert_eq!(range.get_value((1, 2)).unwrap().as_f64(), Some(0.0));
        assert_eq!(range.get_value((1, 3)).unwrap().as_f64(), Some(1.2));

        assert_eq!(range.get_value((2, 0)).unwrap().to_string(), "パックB");
        assert_eq!(range.get_value((2, 1)).unwrap().as_f64(), Some(0.0));
        assert_eq!(range.get_value((2, 2)).unwrap().as_f64(), Some(3.0));

        assert_eq!(range.get_value((3, 0)).unwrap().to_string(), "理性価値");
        assert_eq!(range.get_value((3, 1)).unwrap().as_f64(), Some(0.05));
        assert_eq!(range.get_value((3, 2)).unwrap().as_f64(), Some(0.2));
        assert!(range.get_value((3, 3)).is_none_or(|c| matches!(c, Data::Empty)));
    }
}
