//! 下載的 `Content-Disposition` 與 S3 metadata 的檔名編碼（來源：`energy/attachments/AttachmentService.ts`）。
//!
//! RFC 5987：`filename*` 是正解，`filename` 是給不懂它的 client 的 ASCII 降級，兩個都要給。

/// JS `encodeURIComponent` 不編碼的字元。
fn is_uri_component_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')')
}

/// 與 JS `encodeURIComponent` 相同的百分比編碼（UTF-8 bytes，大寫十六進位）。
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if is_uri_component_unreserved(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// S3 object metadata 只能放 ASCII —— 原檔名以 URL-encoded 存一份（DB 那份才是真值）。
pub fn encode_filename_for_metadata(name: &str) -> String {
    encode_uri_component(name)
}

/// RFC 5987 的 `Content-Disposition`。
///
/// ASCII 降級：可印 ASCII 以外一律換成 `_`（照 JS 以 UTF-16 code unit 計，BMP 外的字變兩個 `_`），
/// 引號與反斜線也換掉，免得打破 quoted-string。
pub fn content_disposition(original_filename: &str) -> String {
    let mut ascii = String::with_capacity(original_filename.len());
    for c in original_filename.chars() {
        match c {
            '"' | '\\' => ascii.push('_'),
            ' '..='~' => ascii.push(c),
            other => ascii.push_str(&"_".repeat(other.len_utf16())),
        }
    }
    format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
        encode_uri_component(original_filename)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 來源：AttachmentService.ts 的 contentDisposition（中文檔名兩種寫法都要有）。
    #[test]
    fn chinese_filename_gets_both_forms() {
        let h = content_disposition("電費單(有餘電需開發票).pdf");
        assert_eq!(
            h,
            "attachment; filename=\"___(_______).pdf\"; filename*=UTF-8''%E9%9B%BB%E8%B2%BB%E5%96%AE(%E6%9C%89%E9%A4%98%E9%9B%BB%E9%9C%80%E9%96%8B%E7%99%BC%E7%A5%A8).pdf"
        );
    }

    #[test]
    fn quotes_and_astral_chars_in_fallback() {
        let h = content_disposition("a\"b\\c 😀.pdf");
        assert!(h.starts_with("attachment; filename=\"a_b_c __.pdf\"; "));
        assert!(h.ends_with("filename*=UTF-8''a%22b%5Cc%20%F0%9F%98%80.pdf"));
        assert_eq!(encode_filename_for_metadata("a b&c'~"), "a%20b%26c'~");
    }
}
