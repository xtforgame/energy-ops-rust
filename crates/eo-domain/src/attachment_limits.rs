//! 附件的白名單與上限（來源：`energy/attachments/AttachmentService.ts` 開頭的常數）。
//!
//! 與辨識服務收得下的型別（`document_ingest::RECOGNIZABLE_MIME_TYPES`）不同：附件收 HEIC。

use serde::{Deserialize, Serialize};

/// 附件類別：封閉代碼，新增一種要同時決定它算不算哪一關的「必傳」對象。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AttachmentCategory {
    /// 台電電費通知單。
    TaipowerNotice,
    /// 存摺影本。
    BankbookCopy,
    /// 台電轉供單清單。
    TransferList,
    /// 核准單影本。
    ApprovalSheet,
    /// 收據。
    Receipt,
    /// 其他（沒有對到上面任何一種）。
    Other,
}

impl AttachmentCategory {
    pub const ALL: [AttachmentCategory; 6] = [
        AttachmentCategory::TaipowerNotice,
        AttachmentCategory::BankbookCopy,
        AttachmentCategory::TransferList,
        AttachmentCategory::ApprovalSheet,
        AttachmentCategory::Receipt,
        AttachmentCategory::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AttachmentCategory::TaipowerNotice => "taipowerNotice",
            AttachmentCategory::BankbookCopy => "bankbookCopy",
            AttachmentCategory::TransferList => "transferList",
            AttachmentCategory::ApprovalSheet => "approvalSheet",
            AttachmentCategory::Receipt => "receipt",
            AttachmentCategory::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

/// 手機拍的收據預設就是 HEIC —— 漏了會被當 bug 回報。
pub const ALLOWED_MIME_TYPES: [&str; 4] = ["application/pdf", "image/jpeg", "image/png", "image/heic"];

/// 彩色掃描 PDF 常見 2–10MB，25MB 有餘裕又擋得住誤傳影片。
pub const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;
/// 純防呆，不是業務規則。
pub const MAX_FILES_PER_EXECUTION: u64 = 50;
pub const MAX_BYTES_PER_EXECUTION: u64 = 200 * 1024 * 1024;

pub fn is_allowed_mime(mime_type: &str) -> bool {
    ALLOWED_MIME_TYPES.contains(&mime_type)
}

/// 儲存 key 的副檔名；不認得的型別是 `bin`。
pub fn ext_for_mime(mime_type: &str) -> &'static str {
    match mime_type {
        "application/pdf" => "pdf",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/heic" => "heic",
        _ => "bin",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 來源：無對應 TS 測試；補驗 AttachmentService.ts 的 ALLOWED_MIME_TYPES／EXT_BY_MIME。
    #[test]
    fn heic_is_an_attachment_but_not_recognizable() {
        assert!(is_allowed_mime("image/heic"));
        assert!(!crate::document_ingest::is_recognizable_mime("image/heic"));
        assert_eq!(ext_for_mime("image/heic"), "heic");
        assert_eq!(ext_for_mime("video/mp4"), "bin");
    }
}
