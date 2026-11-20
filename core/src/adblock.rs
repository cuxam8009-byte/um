//! Bộ lọc quảng cáo Dymith (Phase 0.2).
//!
//! - Đọc luật kiểu Adblock Plus/uBlock: `||host^`, `|prefix`, `suffix|`, `*`, `^`, ngoại lệ `@@`.
//! - Tùy chọn hỗ trợ: third-party/first-party và loại tài nguyên (script, image, stylesheet...).
//!   Luật có tùy chọn lạ (domain=, csp=, redirect=...) bị bỏ qua để tránh chặn nhầm.
//! - Luật thuần tên miền đi đường nhanh (HashSet theo hậu tố nhãn). Luật phức tạp được lập chỉ
//!   mục theo token (FNV-1a), nên mỗi request chỉ phải so khớp vài luật ứng viên.
//! - Engine bất biến sau khi dựng nên đọc không cần khoá; đổi danh sách bằng cách thay cả Engine.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::util::{is_ip_host, parse_url, registrable_domain};

const BUILTIN: &str = include_str!("../data/filters.txt");

// Bit loại tài nguyên (Kotlin suy ra từ đuôi tệp / header Accept; 0 = không rõ).
pub const T_SCRIPT: u32 = 1;
pub const T_IMAGE: u32 = 2;
pub const T_STYLE: u32 = 4;
pub const T_FRAME: u32 = 8;
pub const T_XHR: u32 = 16;
pub const T_MEDIA: u32 = 32;
pub const T_FONT: u32 = 64;
pub const T_OTHER: u32 = 128;

enum Part {
    Lit(String),
    Sep,
    Wild,
}

struct Rule {
    parts: Vec<Part>,
    anchor_host: bool,
    anchor_start: bool,
    anchor_end: bool,
    exception: bool,
    party: u8, // 0: bất kỳ, 1: chỉ bên thứ ba, 2: chỉ bên thứ nhất
    types: u32,
    not_types: u32,
}

struct Compiled {
    parts: Vec<Part>,
    anchor_host: bool,
    anchor_start: bool,
    anchor_end: bool,
}

pub struct Engine {
    domains: HashSet<String>,
    domain_exceptions: HashSet<String>,
    rules: Vec<Rule>,
    index: HashMap<u64, Vec<u32>>,
    untokenized: Vec<u32>,
    pub skipped: usize,
    pub load_micros: u64,
}

fn type_bit(name: &str) -> Option<u32> {
    match name {
        "script" => Some(T_SCRIPT),
        "image" => Some(T_IMAGE),
        "stylesheet" | "css" => Some(T_STYLE),
        "subdocument" | "frame" => Some(T_FRAME),
        "xmlhttprequest" | "xhr" => Some(T_XHR),
        "media" => Some(T_MEDIA),
        "font" => Some(T_FONT),
        "ping" | "other" | "object" | "websocket" | "beacon" => Some(T_OTHER),
        _ => None,
    }
}

fn hosts_entry(line: &str) -> Option<String> {
    let mut it = line.split_whitespace();
    let ip = it.next()?;
    if ip != "0.0.0.0" && ip != "127.0.0.1" {
        return None;
    }
    let host = it.next()?;
    if host == "localhost" || !host.contains('.') {
        return None;
    }
    if !host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
    {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// Luật thuần tên miền: "||host^" hoặc "||host".
fn pure_domain(pat: &str) -> Option<String> {
    let rest = pat.strip_prefix("||")?;
    let host = rest.strip_suffix('^').unwrap_or(rest);
    if host.is_empty() || !host.contains('.') {
        return None;
    }
    if !host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
    {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

fn compile_pattern(pat: &str) -> Option<Compiled> {
    let mut p = pat;
    let mut anchor_host = false;
    let mut anchor_start = false;
    let mut anchor_end = false;
    if let Some(r) = p.strip_prefix("||") {
        anchor_host = true;
        p = r;
    } else if let Some(r) = p.strip_prefix('|') {
        anchor_start = true;
        p = r;
    }
    if let Some(r) = p.strip_suffix('|') {
        anchor_end = true;
        p = r;
    }
    if p.is_empty() {
        return None;
    }
    let mut parts: Vec<Part> = Vec::new();
    let mut cur = String::new();
    for c in p.chars() {
        match c {
            '*' => {
                if !cur.is_empty() {
                    parts.push(Part::Lit(std::mem::take(&mut cur)));
                }
                if !matches!(parts.last(), Some(Part::Wild)) {
                    parts.push(Part::Wild);
                }
            }
            '^' => {
                if !cur.is_empty() {
                    parts.push(Part::Lit(std::mem::take(&mut cur)));
                }
                parts.push(Part::Sep);
            }
            _ => cur.push(c.to_ascii_lowercase()),
        }
    }
    if !cur.is_empty() {
        parts.push(Part::Lit(cur));
    }
    if !anchor_host && !anchor_start {
        while matches!(parts.first(), Some(Part::Wild)) {
            parts.remove(0);
        }
    }
    if !anchor_end {
        while matches!(parts.last(), Some(Part::Wild)) {
            parts.pop();
        }
    }
    if parts.is_empty() {
        return None;
    }
    let has_text = parts
        .iter()
        .any(|x| matches!(x, Part::Lit(s) if s.chars().any(|c| c.is_alphanumeric())));
    if !has_text {
        return None;
    }
    Some(Compiled {
        parts,
        anchor_host,
        anchor_start,
        anchor_end,
    })
}

fn hash_token(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Chọn token dài nhất (>= 3 ký tự chữ-số) nằm gọn giữa hai ranh giới chắc chắn.
fn choose_token(rule: &Rule) -> Option<String> {
    let n = rule.parts.len();
    let mut best: Option<String> = None;
    for (idx, part) in rule.parts.iter().enumerate() {
        let lit = match part {
            Part::Lit(s) => s,
            _ => continue,
        };
        let b = lit.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if !b[i].is_ascii_alphanumeric() {
                i += 1;
                continue;
            }
            let start = i;
            while i < b.len() && b[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let end = i;
            if end - start < 3 {
                continue;
            }
            let left_ok = if start > 0 {
                true
            } else {
                (idx == 0 && (rule.anchor_host || rule.anchor_start))
                    || (idx > 0 && matches!(rule.parts[idx - 1], Part::Sep))
            };
            let right_ok = if end < b.len() {
                true
            } else {
                (idx == n - 1 && rule.anchor_end)
                    || (idx + 1 < n && matches!(rule.parts[idx + 1], Part::Sep))
            };
            if left_ok && right_ok {
                let better = match &best {
                    Some(cur) => end - start > cur.len(),
                    None => true,
                };
                if better {
                    best = Some(lit[start..end].to_string());
                }
            }
        }
    }
    best
}

fn is_sep(b: u8) -> bool {
    !(b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' || b == b'%')
}

fn match_parts(text: &[u8], ti: usize, parts: &[Part], pi: usize, anchor_end: bool) -> bool {
    if pi == parts.len() {
        return !anchor_end || ti == text.len();
    }
    match &parts[pi] {
        Part::Lit(s) => {
            let lit = s.as_bytes();
            if text.len() >= ti + lit.len() && &text[ti..ti + lit.len()] == lit {
                match_parts(text, ti + lit.len(), parts, pi + 1, anchor_end)
            } else {
                false
            }
        }
        Part::Sep => {
            if ti == text.len() {
                match_parts(text, ti, parts, pi + 1, anchor_end)
            } else if is_sep(text[ti]) {
                match_parts(text, ti + 1, parts, pi + 1, anchor_end)
            } else {
                false
            }
        }
        Part::Wild => {
            let mut k = ti;
            loop {
                if match_parts(text, k, parts, pi + 1, anchor_end) {
                    return true;
                }
                if k >= text.len() {
                    return false;
                }
                k += 1;
            }
        }
    }
}

fn pattern_matches(rule: &Rule, lc: &str, host_start: usize, host_end: usize) -> bool {
    let text = lc.as_bytes();
    if rule.anchor_host {
        // Bắt đầu tại đầu host hoặc ngay sau mỗi dấu '.' trong host.
        let mut start = host_start;
        loop {
            if match_parts(text, start, &rule.parts, 0, rule.anchor_end) {
                return true;
            }
            let mut next: Option<usize> = None;
            let mut i = start;
            while i < host_end {
                if text[i] == b'.' {
                    next = Some(i + 1);
                    break;
                }
                i += 1;
            }
            match next {
                Some(n) if n < host_end => start = n,
                _ => return false,
            }
        }
    } else if rule.anchor_start {
        match_parts(text, 0, &rule.parts, 0, rule.anchor_end)
    } else {
        let mut i = 0;
        loop {
            if match_parts(text, i, &rule.parts, 0, rule.anchor_end) {
                return true;
            }
            if i >= text.len() {
                return false;
            }
            i += 1;
        }
    }
}

fn rule_applies(r: &Rule, third_party: Option<bool>, ty: u32) -> bool {
    match r.party {
        1 => {
            if third_party != Some(true) {
                return false;
            }
        }
        2 => {
            if third_party != Some(false) {
                return false;
            }
        }
        _ => {}
    }
    if r.types != 0 && (ty & r.types) == 0 {
        return false;
    }
    if r.not_types != 0 && (ty & r.not_types) != 0 {
        return false;
    }
    true
}

fn suffix_in(set: &HashSet<String>, host: &str) -> bool {
    if set.is_empty() {
        return false;
    }
    let mut cur = host;
    loop {
        if set.contains(cur) {
            return true;
        }
        match cur.split_once('.') {
            Some((_, rest)) if rest.contains('.') => cur = rest,
            _ => return false,
        }
    }
}

fn third_party_of(host: &str, source: &str) -> Option<bool> {
    if source.is_empty() {
        return None;
    }
    let src = source.to_ascii_lowercase();
    let sp = parse_url(&src)?;
    let shost = &src[sp.host_start..sp.host_end];
    Some(registrable_domain(host) != registrable_domain(shost))
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            domains: HashSet::new(),
            domain_exceptions: HashSet::new(),
            rules: Vec::new(),
            index: HashMap::new(),
            untokenized: Vec::new(),
            skipped: 0,
            load_micros: 0,
        }
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    pub fn domain_count(&self) -> usize {
        self.domains.len() + self.domain_exceptions.len()
    }

    pub fn add_list(&mut self, text: &str) {
        let t0 = Instant::now();
        for line in text.lines() {
            self.add_line(line);
        }
        self.load_micros += t0.elapsed().as_micros() as u64;
    }

    fn add_line(&mut self, raw: &str) {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('!') || line.starts_with('[') || line.starts_with('#')
        {
            return;
        }
        // Luật che phần tử / scriptlet (cosmetic): chưa hỗ trợ ở Phase 0.2.
        if line.contains("##")
            || line.contains("#@#")
            || line.contains("#?#")
            || line.contains("#$#")
            || line.contains("#%#")
        {
            self.skipped += 1;
            return;
        }
        if let Some(d) = hosts_entry(line) {
            self.domains.insert(d);
            return;
        }
        let (exception, body) = match line.strip_prefix("@@") {
            Some(r) => (true, r),
            None => (false, line),
        };
        let (pat, opts): (&str, Option<&str>) = match body.rfind('$') {
            Some(i) if !body[i + 1..].contains('/') => (&body[..i], Some(&body[i + 1..])),
            _ => (body, None),
        };
        // Luật regex /.../ chưa hỗ trợ.
        if pat.len() >= 2 && pat.starts_with('/') && pat.ends_with('/') {
            self.skipped += 1;
            return;
        }

        let mut party: u8 = 0;
        let mut types: u32 = 0;
        let mut not_types: u32 = 0;
        if let Some(o) = opts {
            for opt in o.split(',') {
                let opt = opt.trim();
                if opt.is_empty() {
                    continue;
                }
                let (neg, name) = match opt.strip_prefix('~') {
                    Some(n) => (true, n),
                    None => (false, opt),
                };
                match name {
                    "third-party" | "3p" => party = if neg { 2 } else { 1 },
                    "first-party" | "1p" => party = if neg { 1 } else { 2 },
                    "important" => {}
                    _ => match type_bit(name) {
                        Some(bit) => {
                            if neg {
                                not_types |= bit;
                            } else {
                                types |= bit;
                            }
                        }
                        None => {
                            self.skipped += 1;
                            return;
                        }
                    },
                }
            }
        }

        if party == 0 && types == 0 && not_types == 0 {
            if let Some(host) = pure_domain(pat) {
                if exception {
                    self.domain_exceptions.insert(host);
                } else {
                    self.domains.insert(host);
                }
                return;
            }
        }

        let compiled = match compile_pattern(pat) {
            Some(c) => c,
            None => {
                self.skipped += 1;
                return;
            }
        };
        let rule = Rule {
            parts: compiled.parts,
            anchor_host: compiled.anchor_host,
            anchor_start: compiled.anchor_start,
            anchor_end: compiled.anchor_end,
            exception,
            party,
            types,
            not_types,
        };
        let id = self.rules.len() as u32;
        match choose_token(&rule) {
            Some(tok) => self
                .index
                .entry(hash_token(tok.as_bytes()))
                .or_insert_with(Vec::new)
                .push(id),
            None => self.untokenized.push(id),
        }
        self.rules.push(rule);
    }

    fn candidates(&self, lc: &str) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        if self.rules.is_empty() {
            return out;
        }
        let b = lc.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if !b[i].is_ascii_alphanumeric() {
                i += 1;
                continue;
            }
            let start = i;
            while i < b.len() && b[i].is_ascii_alphanumeric() {
                i += 1;
            }
            if i - start >= 3 {
                if let Some(ids) = self.index.get(&hash_token(&b[start..i])) {
                    out.extend_from_slice(ids);
                }
            }
        }
        out.extend_from_slice(&self.untokenized);
        out.sort_unstable();
        out.dedup();
        out
    }

    /// `ty` là một bit T_* (hoặc 0 nếu không rõ); `source` là URL trang đang mở ("" nếu không rõ).
    pub fn should_block(&self, url: &str, source: &str, ty: u32) -> bool {
        let lc = url.to_ascii_lowercase();
        let parts = match parse_url(&lc) {
            Some(p) => p,
            None => return false,
        };
        let scheme = &lc[..parts.scheme_end];
        if scheme != "http" && scheme != "https" {
            return false;
        }
        let host = &lc[parts.host_start..parts.host_end];
        if is_ip_host(host) {
            return false;
        }
        let third_party = third_party_of(host, source);
        if suffix_in(&self.domain_exceptions, host) {
            return false;
        }
        let cands = self.candidates(&lc);
        for &id in &cands {
            let r = &self.rules[id as usize];
            if r.exception
                && rule_applies(r, third_party, ty)
                && pattern_matches(r, &lc, parts.host_start, parts.host_end)
            {
                return false;
            }
        }
        if suffix_in(&self.domains, host) {
            return true;
        }
        for &id in &cands {
            let r = &self.rules[id as usize];
            if !r.exception
                && rule_applies(r, third_party, ty)
                && pattern_matches(r, &lc, parts.host_start, parts.host_end)
            {
                return true;
            }
        }
        false
    }
}

// ------------------------------------------------------------------ engine toàn cục

static ENGINE: Mutex<Option<Arc<Engine>>> = Mutex::new(None);
static CHECKED: AtomicU64 = AtomicU64::new(0);
static BLOCKED: AtomicU64 = AtomicU64::new(0);

fn build_engine(dir: Option<&str>) -> Engine {
    let mut e = Engine::new();
    e.add_list(BUILTIN);
    if let Some(d) = dir {
        let fdir = std::path::Path::new(d).join("filters");
        if let Ok(rd) = std::fs::read_dir(&fdir) {
            for ent in rd.flatten() {
                let p = ent.path();
                let is_txt = p.extension().and_then(|x| x.to_str()) == Some("txt");
                if is_txt {
                    if let Ok(text) = std::fs::read_to_string(&p) {
                        e.add_list(&text);
                    }
                }
            }
        }
    }
    e
}

fn current() -> Arc<Engine> {
    let mut guard = ENGINE.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_none() {
        *guard = Some(Arc::new(build_engine(None)));
    }
    match guard.as_ref() {
        Some(e) => Arc::clone(e),
        None => Arc::new(Engine::new()),
    }
}

/// Dựng lại engine từ luật có sẵn + các tệp *.txt trong `<dir>/filters`.
pub fn init(dir: &str) {
    let e = Arc::new(build_engine(Some(dir)));
    let mut guard = ENGINE.lock().unwrap_or_else(|p| p.into_inner());
    *guard = Some(e);
}

pub fn reload(dir: &str) -> String {
    init(dir);
    stats()
}

pub fn check(url: &str, source: &str, ty: u32) -> bool {
    CHECKED.fetch_add(1, Ordering::Relaxed);
    let blocked = current().should_block(url, source, ty);
    if blocked {
        BLOCKED.fetch_add(1, Ordering::Relaxed);
    }
    blocked
}

pub fn stats() -> String {
    let e = current();
    format!(
        "luật phức tạp {}, tên miền {}, bỏ qua {}, nạp {} µs, đã kiểm {}, đã chặn {}",
        e.rule_count(),
        e.domain_count(),
        e.skipped,
        e.load_micros,
        CHECKED.load(Ordering::Relaxed),
        BLOCKED.load(Ordering::Relaxed)
    )
}

/// Kiểm tra nhanh trên máy thật: "ok" hoặc danh sách lỗi.
pub fn self_test() -> String {
    let e = current();
    let mut bad: Vec<&str> = Vec::new();
    if !e.should_block("https://ad.doubleclick.net/x", "https://news.example.com/", T_SCRIPT) {
        bad.push("doubleclick");
    }
    if e.should_block("https://www.wikipedia.org/", "https://www.wikipedia.org/", 0) {
        bad.push("wikipedia");
    }
    if e.should_block("https://example.com/a.js", "https://example.com/", T_SCRIPT) {
        bad.push("first-party");
    }
    if bad.is_empty() {
        "ok".to_string()
    } else {
        format!("LỖI: {}", bad.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(list: &str) -> Engine {
        let mut e = Engine::new();
        e.add_list(list);
        e
    }

    const SRC: &str = "https://news.example.com/article";

    #[test]
    fn domain_rules_match_subdomains_only_on_label_boundary() {
        let e = engine("||ads.example.net^\n||tracker.io^\n");
        assert!(e.should_block("https://ads.example.net/x.js", SRC, T_SCRIPT));
        assert!(e.should_block("https://a.b.tracker.io/p?q=1", SRC, 0));
        assert!(!e.should_block("https://nottracker.io/", SRC, 0));
        assert!(!e.should_block("https://example.net/ads.example.net", SRC, 0));
    }

    #[test]
    fn hosts_format_and_comments() {
        let e = engine("# comment\n! comment\n0.0.0.0 bad.example.org\n127.0.0.1 localhost\n");
        assert!(e.should_block("http://bad.example.org/", SRC, 0));
        assert!(!e.should_block("http://localhost/", SRC, 0));
    }

    #[test]
    fn exceptions_win() {
        let e = engine("||tracker.net^\n@@||tracker.net^$image\n@@||safe.tracker.net^\n");
        assert!(e.should_block("https://tracker.net/a.js", SRC, T_SCRIPT));
        assert!(!e.should_block("https://tracker.net/a.png", SRC, T_IMAGE));
        assert!(!e.should_block("https://safe.tracker.net/a.js", SRC, T_SCRIPT));
    }

    #[test]
    fn third_party_option() {
        let e = engine("/banner.$third-party\n");
        assert!(e.should_block("https://cdn.other.com/banner.png", SRC, T_IMAGE));
        assert!(!e.should_block("https://cdn.other.com/banner.png", "https://other.com/p", T_IMAGE));
        // Không rõ trang nguồn: luật third-party không áp dụng.
        assert!(!e.should_block("https://cdn.other.com/banner.png", "", T_IMAGE));
    }

    #[test]
    fn type_options() {
        let e = engine("/track.js$script\n/pix.gif$image,third-party\n");
        assert!(e.should_block("https://x.com/track.js", SRC, T_SCRIPT));
        assert!(!e.should_block("https://x.com/track.js", SRC, T_IMAGE));
        assert!(!e.should_block("https://x.com/track.js", SRC, 0));
        assert!(e.should_block("https://x.com/pix.gif", SRC, T_IMAGE));
    }

    #[test]
    fn wildcard_and_separator() {
        let e = engine("||example.org/ads/*/banner^\n");
        assert!(e.should_block("https://example.org/ads/2024/banner?x=1", SRC, 0));
        assert!(e.should_block("https://www.example.org/ads/a/b/banner", SRC, 0));
        assert!(!e.should_block("https://example.org/ads/2024/banner.gif", SRC, 0));
        assert!(!e.should_block("https://example.org/news/2024/banner", SRC, 0));
    }

    #[test]
    fn anchors() {
        let e = engine("|https://exact.example.com/x|\n");
        assert!(e.should_block("https://exact.example.com/x", SRC, 0));
        assert!(!e.should_block("https://exact.example.com/x/y", SRC, 0));
    }

    #[test]
    fn unsupported_rules_are_skipped_not_misapplied() {
        let e = engine("/regex-rule/\n||a.com^$domain=b.com\n##.ad\nexample.org##.banner\n");
        assert_eq!(e.rule_count(), 0);
        assert_eq!(e.domain_count(), 0);
        // "##.ad" bắt đầu bằng '#' nên được coi là dòng chú thích (không tính vào skipped).
        assert_eq!(e.skipped, 3);
        assert!(!e.should_block("https://a.com/", SRC, 0));
    }

    #[test]
    fn builtin_list_works() {
        let mut e = Engine::new();
        e.add_list(BUILTIN);
        assert!(e.should_block("https://securepubads.g.doubleclick.net/x", SRC, T_SCRIPT));
        assert!(e.should_block("https://cdn.thirdparty.com/pagead/js/adsbygoogle.js", SRC, T_SCRIPT));
        assert!(!e.should_block("https://news.example.com/pagead/js/app.js", SRC, T_SCRIPT));
        assert!(!e.should_block("https://www.wikipedia.org/", SRC, 0));
    }
}
