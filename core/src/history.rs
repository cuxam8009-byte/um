//! Lịch sử duyệt web và gợi ý ô địa chỉ (frecency), lưu bằng một tệp TSV nhỏ.
//!
//! Gợi ý khớp không dấu tiếng Việt ("ban hang" khớp "Bán hàng"), mọi từ khoá phải xuất hiện
//! trong URL hoặc tiêu đề. Điểm = ln(1 + số lần truy cập) x độ mới (suy giảm theo ngày) x thưởng
//! tiền tố. Ghi đĩa chỉ khi gọi `flush()` (Kotlin gọi lúc onPause) để không tốn I/O mỗi lượt truy cập.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use crate::util::{fold, parse_url};

const MAX_ENTRIES: usize = 4000;
const DAY_MS: f64 = 86_400_000.0;

struct Entry {
    url: String,
    title: String,
    key: String,       // "host/path?query" hạ chữ thường, bỏ scheme và "www."
    title_key: String, // tiêu đề đã bỏ dấu
    visits: u32,
    last_ms: i64,
}

struct History {
    entries: Vec<Entry>,
    path: Option<PathBuf>,
    dirty: bool,
}

static HISTORY: Mutex<History> = Mutex::new(History {
    entries: Vec::new(),
    path: None,
    dirty: false,
});

fn lock() -> MutexGuard<'static, History> {
    HISTORY.lock().unwrap_or_else(|p| p.into_inner())
}

fn key_of(norm: &str) -> String {
    let rest = match norm.find("://") {
        Some(i) => &norm[i + 3..],
        None => norm,
    };
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    rest.to_lowercase()
}

fn clean(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    out = out.trim().to_string();
    if out.chars().count() > 200 {
        out = out.chars().take(200).collect();
    }
    out
}

/// Chuẩn hoá URL để so trùng: chỉ http(s), bỏ fragment và userinfo, hạ chữ thường scheme/host.
fn normalize(url: &str) -> Option<String> {
    if url.len() > 2048 {
        return None;
    }
    let no_frag = match url.find('#') {
        Some(i) => &url[..i],
        None => url,
    };
    let p = parse_url(no_frag)?;
    let scheme = no_frag[..p.scheme_end].to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = no_frag[p.host_start..p.host_end].to_ascii_lowercase();
    if host.ends_with(".invalid") || host == "appassets.androidplatform.net" {
        return None;
    }
    let tail = &no_frag[p.host_end..];
    Some(format!("{}://{}{}", scheme, host, tail))
}

fn frecency(e: &Entry, now_ms: i64) -> f64 {
    let age_days = ((now_ms - e.last_ms).max(0) as f64) / DAY_MS;
    let recency = 1.0 / (1.0 + age_days / 14.0);
    (1.0 + e.visits as f64).ln() * recency
}

fn prune(entries: &mut Vec<Entry>, now_ms: i64) {
    entries.sort_by(|a, b| {
        frecency(b, now_ms)
            .partial_cmp(&frecency(a, now_ms))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    entries.truncate(MAX_ENTRIES);
}

/// Nạp lịch sử từ `<dir>/history.tsv`; trả về số mục đã nạp.
pub fn init(dir: &str) -> usize {
    let path = PathBuf::from(dir).join("history.tsv");
    let mut loaded: Vec<Entry> = Vec::new();
    if let Ok(text) = fs::read_to_string(&path) {
        for line in text.lines() {
            if loaded.len() >= MAX_ENTRIES {
                break;
            }
            let mut it = line.split('\t');
            let url = match it.next() {
                Some(u) if !u.is_empty() => u,
                _ => continue,
            };
            let title = it.next().unwrap_or("");
            let visits = it.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(1);
            let last_ms = it.next().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
            loaded.push(Entry {
                url: url.to_string(),
                title: title.to_string(),
                key: key_of(url),
                title_key: fold(title),
                visits,
                last_ms,
            });
        }
    }
    let n = loaded.len();
    let mut h = lock();
    h.entries = loaded;
    h.path = Some(path);
    h.dirty = false;
    n
}

pub fn record(url: &str, title: &str, now_ms: i64) {
    let norm = match normalize(url) {
        Some(n) => n,
        None => return,
    };
    let title = clean(title);
    let mut h = lock();
    let pos = h.entries.iter().position(|e| e.url == norm);
    match pos {
        Some(i) => {
            let e = &mut h.entries[i];
            e.visits = e.visits.saturating_add(1);
            e.last_ms = now_ms;
            if !title.is_empty() {
                e.title_key = fold(&title);
                e.title = title;
            }
        }
        None => {
            let entry = Entry {
                key: key_of(&norm),
                title_key: fold(&title),
                url: norm,
                title,
                visits: 1,
                last_ms: now_ms,
            };
            h.entries.push(entry);
        }
    }
    h.dirty = true;
    if h.entries.len() > MAX_ENTRIES + 200 {
        prune(&mut h.entries, now_ms);
    }
}

/// Trả về tối đa `limit` cặp (tiêu đề, URL), điểm cao trước.
pub fn suggest(query: &str, limit: usize, now_ms: i64) -> Vec<(String, String)> {
    let q = fold(query.trim());
    let tokens: Vec<&str> = q.split_whitespace().collect();
    let h = lock();
    let mut scored: Vec<(f64, usize)> = Vec::new();
    for (i, e) in h.entries.iter().enumerate() {
        let mut ok = true;
        let mut boost = 1.0f64;
        for (ti, &t) in tokens.iter().enumerate() {
            let in_key = e.key.contains(t);
            let in_title = e.title_key.contains(t);
            if !in_key && !in_title {
                ok = false;
                break;
            }
            if ti == 0 && (e.key.starts_with(t) || e.title_key.starts_with(t)) {
                boost = 2.0;
            }
        }
        if ok {
            scored.push((frecency(e, now_ms) * boost, i));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let out: Vec<(String, String)> = scored
        .into_iter()
        .take(limit)
        .map(|(_, i)| {
            let e = &h.entries[i];
            let title = if e.title.is_empty() {
                e.key.clone()
            } else {
                e.title.clone()
            };
            (title, e.url.clone())
        })
        .collect();
    out
}

pub fn flush() {
    let (path, data) = {
        let mut h = lock();
        if !h.dirty {
            return;
        }
        let path = match &h.path {
            Some(p) => p.clone(),
            None => return,
        };
        let mut s = String::with_capacity(h.entries.len() * 80);
        for e in &h.entries {
            s.push_str(&e.url);
            s.push('\t');
            s.push_str(&e.title);
            s.push('\t');
            s.push_str(&e.visits.to_string());
            s.push('\t');
            s.push_str(&e.last_ms.to_string());
            s.push('\n');
        }
        h.dirty = false;
        (path, s)
    };
    let tmp = path.with_extension("tsv.tmp");
    if let Ok(mut f) = fs::File::create(&tmp) {
        if f.write_all(data.as_bytes()).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

pub fn clear() {
    {
        let mut h = lock();
        h.entries.clear();
        h.dirty = true;
    }
    flush();
}

pub fn stats() -> String {
    let n = lock().entries.len();
    format!("lịch sử {} mục", n)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    // Bộ nhớ lịch sử là toàn cục nên gom mọi kiểm tra vào một test để tránh chạy song song.
    #[test]
    fn history_flow() {
        clear();
        record("https://vnexpress.net/the-thao?x=1#top", "Thể thao - VnExpress", NOW);
        record("https://vnexpress.net/the-thao?x=1", "Thể thao - VnExpress", NOW + 1000);
        record("https://example.com/blog/ban-hang", "Bán hàng online hiệu quả", NOW);
        record("about:blank", "", NOW);
        record("https://appassets.androidplatform.net/", "Trang chủ", NOW);

        // URL chỉ khác fragment được gộp, mục lạ (about:blank, trang chủ ảo) bị bỏ
        let all = suggest("", 10, NOW + 2000);
        assert_eq!(all.len(), 2);

        let r = suggest("the thao", 5, NOW + 2000);
        assert_eq!(r.len(), 1);
        assert!(r[0].1.starts_with("https://vnexpress.net/"));

        let r2 = suggest("ban hang", 5, NOW + 2000);
        assert_eq!(r2.len(), 1);
        assert_eq!(r2[0].0, "Bán hàng online hiệu quả");

        assert!(suggest("khong co", 5, NOW).is_empty());
        clear();
    }

    #[test]
    fn normalize_rules() {
        assert_eq!(
            normalize("HTTPS://User@Example.COM:8080/a/B?q=1#f").unwrap(),
            "https://example.com:8080/a/B?q=1"
        );
        assert!(normalize("ftp://x.y/").is_none());
        assert!(normalize("https://a.invalid/").is_none());
        assert!(normalize("https://appassets.androidplatform.net/").is_none());
    }
}
