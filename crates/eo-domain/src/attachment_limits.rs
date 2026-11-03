//! 附件的白名單與上限（來源：`energy/attachments/AttachmentService.ts` 開頭的常數）。
//!
//! 與辨識服務收得下的型別（`document_ingest::RECOGNIZABLE_MIME_TYPES`）不同：附件收 HEIC。

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
