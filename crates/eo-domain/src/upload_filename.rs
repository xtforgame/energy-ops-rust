//! multipart 上傳檔名的解碼 —— 只有一份（來源：`energy/attachments/uploadFilename.ts`）。
//!
//! 舊 parser 把 UTF-8 bytes 當 latin1 讀（一個 byte 一個字元），新 parser 給的已是正確字串。
//! 判準：有 latin1 放不下的字（> U+00FF）＝ 已經解好了，原樣返回；
//! 否則把每個字當一個 byte 轉回 UTF-8，轉不出合法 UTF-8（出現 U+FFFD）就代表本來就是 latin1 檔名。
//! 對已解好的中文硬轉會截出控制碼甚至 NUL，Postgres jsonb 拒收 → 稽核 fail-closed → 上傳 500。

/// 解碼上傳檔名；None／空字串回空字串。
pub fn decode_upload_filename(raw: Option<&str>) -> String {
    let s = raw.unwrap_or("");
    if s.is_empty() {
        return String::new();
    }
    // 範圍從 U+0000 起算：mangled 字串裡本來就有控制碼，那些正是最該修的。
    if s.chars().any(|c| c as u32 > 0xFF) {
        return s.to_string();
    }
    let bytes: Vec<u8> = s.chars().map(|c| c as u32 as u8).collect();
    let decoded = String::from_utf8_lossy(&bytes);
    if decoded.contains('\u{FFFD}') {
        s.to_string()
    } else {
        decoded.into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: [&str; 4] = [
        "電費單(有餘電需開發票).pdf",
        "電費單(無餘電需付款).pdf",
        "逆變器電費單(僅設備發電使用電費).pdf",
        "逆變器電費單(含電表租費&設備發電使用電費).pdf",
    ];

    /// 模擬 busboy 0.x：把 UTF-8 bytes 當 latin1 讀進來。
    fn as_latin1(name: &str) -> String {
        name.bytes().map(char::from).collect()
    }

    /// 模擬 `Buffer.from(s, 'latin1')`：每個 UTF-16 code unit 只留低位 byte。
    fn naive_latin1_bytes(s: &str) -> Vec<u8> {
        s.encode_utf16().map(|u| (u & 0xFF) as u8).collect()
    }

    /// 來源：uploadFilename.test.ts「已經解好的中文檔名**原樣返回**（這一版的 parser 給的就是這種）」
    #[test]
    fn decoded_names_are_returned_as_is() {
        for name in REAL {
            assert_eq!(decode_upload_filename(Some(name)), name);
        }
    }

    /// 來源：uploadFilename.test.ts「被當成 latin1 讀進來的中文檔名會被還原」
    #[test]
    fn latin1_mangled_names_are_restored() {
        for name in REAL {
            assert_eq!(decode_upload_filename(Some(&as_latin1(name))), name);
        }
    }

    /// 來源：uploadFilename.test.ts「純 ASCII 檔名不受影響」
    #[test]
    fn ascii_names_are_untouched() {
        for name in ["bill.pdf", "a b (1).PDF", "invoice_2026-08.pdf"] {
            assert_eq!(decode_upload_filename(Some(name)), name);
        }
    }

    /// 來源：uploadFilename.test.ts「結果裡不可以有 NUL —— jsonb 收不下，稽核會 fail-closed 讓上傳 500」
    #[test]
    fn result_never_contains_nul() {
        for name in REAL {
            assert!(!decode_upload_filename(Some(name)).contains('\0'), "{name}");
        }
        // 證明「無條件轉換」確實會產生 NUL（＝當初 500 的原因）。
        let naive = String::from_utf8_lossy(&naive_latin1_bytes("電費單(有餘電需開發票).pdf")).into_owned();
        assert!(naive.contains('\0'));
    }

    /// 來源：uploadFilename.test.ts「空值不炸」
    #[test]
    fn empty_values_do_not_panic() {
        assert_eq!(decode_upload_filename(None), "");
        assert_eq!(decode_upload_filename(Some("")), "");
    }
}
