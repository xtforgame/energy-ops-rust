//! 能源營運的純邏輯：型別、判定、計算。不碰 IO。
//!
//! 每個模組對應舊 TS 的一支來源檔；語意照原檔，差異寫在各模組開頭。

/// 「已知值＋原樣保留未知值」的字串列舉：服務那側的值域會長，不認得的值不可讓整份反序列化失敗。
macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $text:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(from = "String", into = "String")]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
            /// 不在已知值域內的原字串。
            Other(String),
        }

        impl $name {
            pub fn as_str(&self) -> &str {
                match self {
                    $( Self::$variant => $text, )+
                    Self::Other(s) => s.as_str(),
                }
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                match s.as_str() {
                    $( $text => Self::$variant, )+
                    _ => Self::Other(s),
                }
            }
        }

        impl From<$name> for String {
            fn from(v: $name) -> String {
                v.as_str().to_string()
            }
        }
    };
}

pub(crate) mod js_compat;
pub(crate) mod serde_util;

pub mod attachment_limits;
pub mod audit_events;
pub mod bill_extractor;
pub mod business_key;
pub mod conditions;
pub mod content_disposition;
pub mod display_id;
pub mod document_ingest;
pub mod energy_rbac;
pub mod easyflow;
pub mod geocoder;
pub mod rating;
pub mod rbac;
pub mod resolve_mode;
pub mod resource_identifier;
pub mod route_preview;
pub mod schemas;
pub mod side_effects;
pub mod taipower_canonical;
pub mod taipower_extractor;
pub mod upload_filename;
pub mod workflow_defs;
