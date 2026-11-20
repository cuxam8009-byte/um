//! Bộ chặn quảng cáo mẫu cho Phase 0: khớp hậu tố theo ranh giới nhãn tên miền.
//!
//! Mục đích: đo chi phí gọi JNI trên đường nóng (mỗi request con) và cho thấy tác dụng
//! thực tế. Phase 1 sẽ thay bằng `adblock-rust` (FlatBuffers, hàng chục nghìn luật).

use std::collections::HashSet;
use std::sync::OnceLock;

const BLOCKED: &[&str] = &[
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "adservice.google.com",
    "2mdn.net",
    "adnxs.com",
    "taboola.com",
    "outbrain.com",
    "criteo.com",
    "criteo.net",
    "amazon-adsystem.com",
    "adsrvr.org",
    "pubmatic.com",
    "rubiconproject.com",
    "openx.net",
    "casalemedia.com",
    "moatads.com",
    "scorecardresearch.com",
    "quantserve.com",
    "advertising.com",
    "adform.net",
    "smartadserver.com",
    "serving-sys.com",
    "yieldmo.com",
    "zedo.com",
    "popads.net",
    "propellerads.com",
    "exoclick.com",
    "adsterra.com",
    "mgid.com",
    "revcontent.com",
    "media.net",
    "ads.yahoo.com",
    "adcolony.com",
    "applovin.com",
    "mopub.com",
    "inmobi.com",
];

fn blocked_set() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| BLOCKED.iter().copied().collect())
}

/// Lấy host từ URL (bỏ userinfo, cổng; bỏ qua IPv6 literal).
fn host_of(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("://")?;
    let end = rest
        .find(|c: char| c == '/' || c == '?' || c == '#')
        .unwrap_or(rest.len());
    let authority = &rest[..end];
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if authority.starts_with('[') {
        return None;
    }
    let host = authority.split(':').next().unwrap_or(authority);
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn matches_suffix(host: &str) -> bool {
    let blocked = blocked_set();
    let mut cur = host;
    loop {
        if blocked.contains(cur) {
            return true;
        }
        match cur.split_once('.') {
            Some((_, rest)) if rest.contains('.') => cur = rest,
            _ => return false,
        }
    }
}

/// `true` nếu URL thuộc một mạng quảng cáo trong danh sách.
pub fn should_block(url: &str) -> bool {
    let host = match host_of(url) {
        Some(h) => h,
        None => return false,
    };
    if host.bytes().any(|b| b.is_ascii_uppercase()) {
        return matches_suffix(&host.to_ascii_lowercase());
    }
    matches_suffix(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_exact_and_subdomains() {
        assert!(should_block("https://doubleclick.net/x"));
        assert!(should_block("https://ad.doubleclick.net/x?y=1"));
        assert!(should_block("https://a.b.c.googlesyndication.com:443/pagead"));
        assert!(should_block("https://AD.DoubleClick.NET/"));
        assert!(should_block("https://adservice.google.com/ads"));
    }

    #[test]
    fn does_not_block_lookalikes_or_normal_sites() {
        assert!(!should_block("https://notdoubleclick.net/"));
        assert!(!should_block("https://example.com/doubleclick.net"));
        assert!(!should_block("https://www.google.com/search?q=a"));
        assert!(!should_block("https://user@example.com:8080/"));
        assert!(!should_block("data:text/plain,hello"));
    }

    #[test]
    fn host_parsing() {
        assert_eq!(host_of("https://u:p@a.b.com:8443/x"), Some("a.b.com"));
        assert_eq!(host_of("https://[::1]:80/"), None);
        assert_eq!(host_of("about:blank"), None);
    }
}
