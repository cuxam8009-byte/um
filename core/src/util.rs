//! Tiện ích dùng chung: phân tích URL nhẹ, miền đăng ký (xấp xỉ eTLD+1), bỏ dấu tiếng Việt.

/// Vị trí (chỉ số byte) các thành phần của một URL tuyệt đối.
pub struct UrlParts {
    pub scheme_end: usize,
    pub host_start: usize,
    pub host_end: usize,
}

pub fn parse_url(url: &str) -> Option<UrlParts> {
    let scheme_end = url.find("://")?;
    let auth_start = scheme_end + 3;
    let rest = &url[auth_start..];
    let auth_len = rest
        .find(|c: char| c == '/' || c == '?' || c == '#')
        .unwrap_or(rest.len());
    let auth_end = auth_start + auth_len;
    let authority = &url[auth_start..auth_end];
    let host_from = match authority.rfind('@') {
        Some(i) => auth_start + i + 1,
        None => auth_start,
    };
    let hostport = &url[host_from..auth_end];
    let host_end = if hostport.starts_with('[') {
        match hostport.find(']') {
            Some(i) => host_from + i + 1,
            None => auth_end,
        }
    } else {
        match hostport.rfind(':') {
            Some(i) => host_from + i,
            None => auth_end,
        }
    };
    if host_end <= host_from {
        return None;
    }
    Some(UrlParts {
        scheme_end,
        host_start: host_from,
        host_end,
    })
}

pub fn is_ip_host(host: &str) -> bool {
    if host.contains(':') || host.starts_with('[') {
        return true;
    }
    !host.is_empty() && host.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

/// Hậu tố hai tầng phổ biến (không có danh sách PSL đầy đủ ở Phase 0).
const TWO_LEVEL: &[&str] = &[
    "com.vn", "net.vn", "org.vn", "edu.vn", "gov.vn", "info.vn", "biz.vn", "co.uk", "org.uk",
    "ac.uk", "gov.uk", "com.au", "net.au", "org.au", "co.jp", "or.jp", "ne.jp", "co.kr",
    "or.kr", "com.br", "com.cn", "com.tw", "com.hk", "com.sg", "com.my", "co.id", "co.in",
    "co.nz", "co.za", "com.mx", "com.ar", "com.tr", "com.ua", "co.th", "in.th",
];

/// Miền đăng ký xấp xỉ: "a.b.example.com.vn" -> "example.com.vn".
pub fn registrable_domain(host: &str) -> &str {
    if is_ip_host(host) {
        return host;
    }
    let last_dot = match host.rfind('.') {
        Some(i) => i,
        None => return host,
    };
    let second = match host[..last_dot].rfind('.') {
        Some(i) => i,
        None => return host,
    };
    let last_two = &host[second + 1..];
    if TWO_LEVEL.contains(&last_two) {
        match host[..second].rfind('.') {
            Some(i3) => &host[i3 + 1..],
            None => host,
        }
    } else {
        last_two
    }
}

const FOLD_TABLE: [(char, &str); 7] = [
    ('a', "àáạảãâầấậẩẫăằắặẳẵ"),
    ('e', "èéẹẻẽêềếệểễ"),
    ('i', "ìíịỉĩ"),
    ('o', "òóọỏõôồốộổỗơờớợởỡ"),
    ('u', "ùúụủũưừứựửữ"),
    ('y', "ỳýỵỷỹ"),
    ('d', "đ"),
];

fn fold_char(c: char) -> char {
    for (base, set) in FOLD_TABLE.iter() {
        if set.contains(c) {
            return *base;
        }
    }
    c
}

/// Hạ chữ thường và bỏ dấu tiếng Việt ("Bán Hàng Đẹp" -> "ban hang dep").
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
            continue;
        }
        let cp = c as u32;
        if (0x0300..=0x036F).contains(&cp) {
            continue; // dấu kết hợp (NFD)
        }
        for lc in c.to_lowercase() {
            out.push(fold_char(lc));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hosts() {
        let u = "https://user:pw@a.b.com:8443/x?y=1";
        let p = parse_url(u).unwrap();
        assert_eq!(&u[p.host_start..p.host_end], "a.b.com");
        let v6 = "http://[::1]:80/";
        let p6 = parse_url(v6).unwrap();
        assert_eq!(&v6[p6.host_start..p6.host_end], "[::1]");
        assert!(parse_url("about:blank").is_none());
    }

    #[test]
    fn registrable() {
        assert_eq!(registrable_domain("a.b.example.com"), "example.com");
        assert_eq!(registrable_domain("example.com"), "example.com");
        assert_eq!(registrable_domain("news.zing.com.vn"), "zing.com.vn");
        assert_eq!(registrable_domain("192.168.1.1"), "192.168.1.1");
    }

    #[test]
    fn folds_vietnamese() {
        assert_eq!(fold("Bán Hàng Đẹp"), "ban hang dep");
        assert_eq!(fold("Việt Nam"), "viet nam");
        assert_eq!(fold("ƯỚC MƠ"), "uoc mo");
    }
}
