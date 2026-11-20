//! Phân giải ô địa chỉ: quyết định đầu vào là URL hay câu tìm kiếm.
//!
//! Phase 0 dùng heuristic thuần std (không phụ thuộc crate nào). Phase 1 sẽ thay bằng
//! danh sách hậu tố công khai (PSL) để nhận diện tên miền chính xác hơn.

const DEFAULT_TEMPLATE: &str = "https://www.google.com/search?q=%s";

/// Đuôi tệp phổ biến KHÔNG phải TLD: "main.js", "notes.txt"... được coi là câu tìm kiếm.
const NOT_TLDS: &[&str] = &[
    "js", "ts", "jsx", "tsx", "json", "txt", "yml", "yaml", "toml", "html", "htm", "css", "png",
    "jpg", "jpeg", "gif", "svg", "pdf", "zip", "exe", "apk", "java", "kt", "php", "rb", "cpp",
    "hpp", "log", "lock",
];

/// Biến chuỗi người dùng gõ thành URL có thể nạp.
pub fn resolve(input: &str, template: &str) -> String {
    let s = input.trim();
    if s.is_empty() {
        return "about:blank".to_string();
    }
    if let Some(url) = as_url(s) {
        return url;
    }
    search_url(s, template)
}

fn search_url(query: &str, template: &str) -> String {
    let tpl = if template.contains("%s") {
        template
    } else {
        DEFAULT_TEMPLATE
    };
    tpl.replacen("%s", &percent_encode(query), 1)
}

fn as_url(s: &str) -> Option<String> {
    if s.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    if s.eq_ignore_ascii_case("about:blank") {
        return Some("about:blank".to_string());
    }

    // Có scheme rõ ràng: chỉ nhận http/https.
    if let Some(pos) = s.find("://") {
        let scheme = &s[..pos];
        let is_scheme = !scheme.is_empty()
            && scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.');
        if is_scheme {
            let http = scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
            if http && s.len() > pos + 3 {
                return Some(s.to_string());
            }
            return None;
        }
    }

    // host[:port][/path][?query][#fragment]
    let end = s
        .find(|c: char| c == '/' || c == '?' || c == '#')
        .unwrap_or(s.len());
    let authority = &s[..end];
    let rest = &s[end..];
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = split_host_port(authority)?;
    if let Some(p) = port {
        if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        match p.parse::<u32>() {
            Ok(n) if (1..=65535).contains(&n) => {}
            _ => return None,
        }
    }

    let local = is_local_host(host);
    if !local && !is_domain(host) {
        return None;
    }
    let scheme = if local { "http" } else { "https" };
    Some(format!("{}://{}{}", scheme, authority, rest))
}

fn split_host_port(authority: &str) -> Option<(&str, Option<&str>)> {
    if let Some(rest) = authority.strip_prefix('[') {
        // IPv6: [::1] hoặc [::1]:8080
        let close = rest.find(']')?;
        let host = &authority[..close + 2];
        let after = &rest[close + 1..];
        if after.is_empty() {
            return Some((host, None));
        }
        let port = after.strip_prefix(':')?;
        return Some((host, Some(port)));
    }
    match authority.rfind(':') {
        Some(i) => Some((&authority[..i], Some(&authority[i + 1..]))),
        None => Some((authority, None)),
    }
}

fn is_local_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if host.len() > 2 && host.starts_with('[') && host.ends_with(']') {
        return true;
    }
    if is_ipv4(host) {
        return true;
    }
    let h = host.to_ascii_lowercase();
    h.ends_with(".local")
        || h.ends_with(".lan")
        || h.ends_with(".internal")
        || h.ends_with(".test")
        || h.ends_with(".localhost")
}

fn is_ipv4(host: &str) -> bool {
    let mut parts = 0;
    for part in host.split('.') {
        parts += 1;
        if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
        match part.parse::<u16>() {
            Ok(v) if v <= 255 => {}
            _ => return false,
        }
    }
    parts == 4
}

fn is_domain(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    for label in &labels {
        if label.is_empty() || label.chars().count() > 63 {
            return false;
        }
        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }
        if !label
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return false;
        }
    }
    let tld = labels[labels.len() - 1];
    if tld.chars().count() < 2 {
        return false;
    }
    let tld_lower = tld.to_lowercase();
    if NOT_TLDS.contains(&tld_lower.as_str()) {
        return false;
    }
    tld.chars().all(|c| c.is_alphabetic()) || tld_lower.starts_with("xn--")
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

fn percent_encode(q: &str) -> String {
    let mut out = String::with_capacity(q.len() * 3);
    for b in q.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 15) as usize] as char);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "https://www.google.com/search?q=%s";

    fn r(s: &str) -> String {
        resolve(s, T)
    }

    #[test]
    fn empty_is_blank() {
        assert_eq!(r("   "), "about:blank");
    }

    #[test]
    fn full_urls_pass_through() {
        assert_eq!(r("https://example.com/a?b=c"), "https://example.com/a?b=c");
        assert_eq!(r("HTTP://Example.com"), "HTTP://Example.com");
    }

    #[test]
    fn bare_domain_gets_https() {
        assert_eq!(r("example.com"), "https://example.com");
        assert_eq!(r("vnexpress.net/the-thao?x=1#top"), "https://vnexpress.net/the-thao?x=1#top");
        assert_eq!(r("bánhàng.vn"), "https://bánhàng.vn");
    }

    #[test]
    fn localhost_and_ip_use_http() {
        assert_eq!(r("localhost:8080/x"), "http://localhost:8080/x");
        assert_eq!(r("192.168.1.1"), "http://192.168.1.1");
        assert_eq!(r("[::1]:3000"), "http://[::1]:3000");
    }

    #[test]
    fn plain_words_and_sentences_are_searches() {
        assert_eq!(r("dymith"), "https://www.google.com/search?q=dymith");
        assert_eq!(
            r("bán hàng online"),
            "https://www.google.com/search?q=b%C3%A1n+h%C3%A0ng+online"
        );
    }

    #[test]
    fn risky_inputs_are_searches() {
        assert_eq!(r("a@b.com"), "https://www.google.com/search?q=a%40b.com");
        assert_eq!(
            r("javascript:alert(1)"),
            "https://www.google.com/search?q=javascript%3Aalert%281%29"
        );
        assert_eq!(r("main.js"), "https://www.google.com/search?q=main.js");
    }

    #[test]
    fn scheme_inside_query_is_not_a_scheme() {
        assert_eq!(r("example.com/?u=http://x.y"), "https://example.com/?u=http://x.y");
    }

    #[test]
    fn custom_template_is_used() {
        assert_eq!(resolve("abc def", "https://duckduckgo.com/?q=%s"), "https://duckduckgo.com/?q=abc+def");
        assert_eq!(resolve("abc", "https://bad-template"), "https://www.google.com/search?q=abc");
    }
}
