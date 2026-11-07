//! エンドポイントの定義（`ENDPOINTS` が単一の正本）と lists の種類。

use crate::engine::risei_calculator_engine::CC_NUMBER;
use serde_json::{json, Value};
use super::operator_cost;

pub(super) struct ParamSpec {
    pub(super) name: &'static str,
    pub(super) required: bool,
    pub(super) description: &'static str,
}

pub(super) struct EndpointSpec {
    pub(super) path: &'static str,
    pub(super) description: &'static str,
    pub(super) params: &'static [ParamSpec],
}

const SERVER_PARAM: ParamSpec = ParamSpec {
    name: "server",
    required: false,
    description: "global(既定。グローバル版=日本版基準) / mainland(大陸版基準。新ステージ・新素材込み)",
};

const LIMIT_PARAM: ParamSpec = ParamSpec {
    name: "limit",
    required: false,
    description: "返すステージ数の上限(省略時は全件)",
};

const INDEX: EndpointSpec = EndpointSpec {
    path: "/api/risei",
    description: "このAPIの使い方一覧(エンドポイント・引数・素材カテゴリ・効率表の種類・最終更新時刻)",
    params: &[],
};

pub(super) const MATERIALS: EndpointSpec = EndpointSpec {
    path: "/api/risei/materials",
    description: "昇進素材カテゴリ別の効率の良い恒常ステージ(/riseimaterials)。総合効率の降順。\
                  大陸版先行カテゴリ(new)を指定すると自動的に大陸版基準になる",
    params: &[
        ParamSpec {
            name: "target",
            required: true,
            description: "素材カテゴリ。日本語名(例: 砥石)またはキー(中国語名。例: 研磨石)。一覧は GET /api/risei の material_categories",
        },
        SERVER_PARAM,
        LIMIT_PARAM,
    ],
};

pub(super) const STAGES: EndpointSpec = EndpointSpec {
    path: "/api/risei/stages",
    description: "恒常ステージ(メイン・恒常サイドストーリー)の効率(/riseistages)。stageは前方一致で名前順。\
                  グローバル版に無いステージは大陸版へフォールバックする(effective_serverで判別)",
    params: &[
        ParamSpec {
            name: "stage",
            required: true,
            description: "ステージ名(前方一致。大文字小文字を区別する。例: 1-7, 12-17, R8-)",
        },
        SERVER_PARAM,
        LIMIT_PARAM,
    ],
};

pub(super) const EVENTS: EndpointSpec = EndpointSpec {
    path: "/api/risei/events",
    description: "期間限定イベントステージの効率(/riseievents)。過去の開催済み・未開催イベントも対象。\
                  stageは前方一致で名前順",
    params: &[
        ParamSpec {
            name: "stage",
            required: true,
            description: "ステージ名(前方一致。大文字小文字を区別する。例: SV-8, IW-)",
        },
        SERVER_PARAM,
        LIMIT_PARAM,
    ],
};

pub(super) const LISTS: EndpointSpec = EndpointSpec {
    path: "/api/risei/lists/{kind}",
    description: "各種効率表(/riseilists)。kindは list_kinds 参照。効率表は効率の降順。\
                  kind=kakin は課金パック効率(/riseikakin の全体比較)で、server=global のみ対応",
    params: &[
        SERVER_PARAM,
        ParamSpec {
            name: "upcoming",
            required: false,
            description: "kind=kakin のみ。true でまだ販売前のパックも含める(既定 false=販売中だけ)",
        },
    ],
};

pub(super) const ENDPOINTS: &[&EndpointSpec] = &[
    &INDEX,
    &MATERIALS,
    &STAGES,
    &EVENTS,
    &LISTS,
    &operator_cost::OPERATOR_MASTERY,
    &operator_cost::OPERATOR_ELITE,
    &operator_cost::OPERATOR_MODULE,
    &operator_cost::OPERATOR_LISTS,
];

/// `/api/risei/lists/{kind}`の種類（`bot/commands/risei/riseilists.rs`の`RiseiListTarget`相当）。
#[derive(Clone, Copy)]
pub(super) enum ListKind {
    BaseMaps,
    Values,
    Te2,
    Te3,
    Special,
    Cc,
    Kakin,
}

impl ListKind {
    pub(super) const ALL: [ListKind; 7] =
        [Self::BaseMaps, Self::Values, Self::Te2, Self::Te3, Self::Special, Self::Cc, Self::Kakin];

    pub(super) fn key(self) -> &'static str {
        match self {
            Self::BaseMaps => "base_maps",
            Self::Values => "values",
            Self::Te2 => "te2",
            Self::Te3 => "te3",
            Self::Special => "special",
            Self::Cc => "cc",
            Self::Kakin => "kakin",
        }
    }

    pub(super) fn title(self) -> String {
        match self {
            Self::BaseMaps => "基準マップ(素材カテゴリ→基準ステージ)".to_string(),
            Self::Values => "理性価値表(アイテムごとの理性価値±標準偏差)".to_string(),
            Self::Te2 => "初級資格証効率表".to_string(),
            Self::Te3 => "上級資格証効率表".to_string(),
            Self::Special => "特別引換証効率表".to_string(),
            Self::Cc => format!("契約賞金引換効率表(CC#{CC_NUMBER})"),
            Self::Kakin => "課金パック効率(グローバル版。期間限定パックを総合効率の降順＋比較用の恒常パック)".to_string(),
        }
    }

    pub(super) fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

pub(super) fn endpoint_usage(endpoint: &EndpointSpec) -> Value {
    json!({
        "path": endpoint.path,
        "method": "GET",
        "description": endpoint.description,
        "params": endpoint
            .params
            .iter()
            .map(|p| json!({ "name": p.name, "required": p.required, "description": p.description }))
            .collect::<Vec<_>>(),
    })
}
