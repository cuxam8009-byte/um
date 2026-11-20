//! Dymith Core, Phase 0.2.
//!
//! Lõi Rust của trình duyệt, nói chuyện với vỏ Kotlin qua JNI:
//! - `omnibox`: phân giải ô địa chỉ (URL hay câu tìm kiếm)
//! - `adblock`: bộ lọc quảng cáo kiểu Adblock Plus, chỉ mục theo token
//! - `history`: lịch sử và gợi ý địa chỉ theo frecency, khớp không dấu tiếng Việt
#![allow(dead_code)]

mod adblock;
mod history;
mod omnibox;
mod util;

use std::panic::{catch_unwind, AssertUnwindSafe};

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Chuỗi mô tả bản dựng, hiển thị trong màn hình chẩn đoán.
pub fn build_info() -> String {
    let sha = option_env!("DYMITH_GIT_SHA").unwrap_or("dev");
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    format!(
        "dymith-core {}+{} ({}, {})",
        VERSION,
        sha,
        std::env::consts::ARCH,
        profile
    )
}

fn to_jstring(env: &mut JNIEnv, value: &str) -> jstring {
    match env.new_string(value) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

fn from_jstring(env: &mut JNIEnv, value: &JString) -> String {
    match env.get_string(value) {
        Ok(s) => s.into(),
        Err(_) => String::new(),
    }
}

fn json_escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
}

/// [{"t":"tiêu đề","u":"url"}, ...]
fn suggestions_json(items: &[(String, String)]) -> String {
    let mut out = String::from("[");
    for (i, (title, url)) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"t\":\"");
        json_escape(title, &mut out);
        out.push_str("\",\"u\":\"");
        json_escape(url, &mut out);
        out.push_str("\"}");
    }
    out.push(']');
    out
}

#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeVersion<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    let text = build_info();
    to_jstring(&mut env, &text)
}

#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeResolve<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    input: JString<'local>,
    template: JString<'local>,
) -> jstring {
    let input = from_jstring(&mut env, &input);
    let template = from_jstring(&mut env, &template);
    let out = catch_unwind(AssertUnwindSafe(|| omnibox::resolve(&input, &template)))
        .unwrap_or_else(|_| String::from("about:blank"));
    to_jstring(&mut env, &out)
}

/// Bản cũ (1 tham số), giữ lại để tương thích.
#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeShouldBlock<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    url: JString<'local>,
) -> jboolean {
    let url = from_jstring(&mut env, &url);
    let blocked = catch_unwind(AssertUnwindSafe(|| adblock::check(&url, "", 0))).unwrap_or(false);
    if blocked {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeShouldBlockEx<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    url: JString<'local>,
    source: JString<'local>,
    res_type: jint,
) -> jboolean {
    let url = from_jstring(&mut env, &url);
    let source = from_jstring(&mut env, &source);
    let blocked = catch_unwind(AssertUnwindSafe(|| {
        adblock::check(&url, &source, res_type as u32)
    }))
    .unwrap_or(false);
    if blocked {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeInit<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    files_dir: JString<'local>,
) -> jint {
    let dir = from_jstring(&mut env, &files_dir);
    let n = catch_unwind(AssertUnwindSafe(|| {
        adblock::init(&dir);
        history::init(&dir)
    }))
    .unwrap_or(0);
    n as jint
}

#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeRecordVisit<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    url: JString<'local>,
    title: JString<'local>,
    now_ms: jlong,
) {
    let url = from_jstring(&mut env, &url);
    let title = from_jstring(&mut env, &title);
    let _ = catch_unwind(AssertUnwindSafe(|| history::record(&url, &title, now_ms)));
}

#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeSuggest<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    query: JString<'local>,
    limit: jint,
    now_ms: jlong,
) -> jstring {
    let query = from_jstring(&mut env, &query);
    let n = if limit < 1 { 1 } else { limit as usize };
    let json = catch_unwind(AssertUnwindSafe(|| {
        suggestions_json(&history::suggest(&query, n, now_ms))
    }))
    .unwrap_or_else(|_| String::from("[]"));
    to_jstring(&mut env, &json)
}

#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeFlush<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| history::flush()));
}

#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeClearHistory<'local>(
    _env: JNIEnv<'local>,
    _class: JClass<'local>,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| history::clear()));
}

#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeStats<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    let text = catch_unwind(AssertUnwindSafe(|| {
        format!("{} | {}", adblock::stats(), history::stats())
    }))
    .unwrap_or_else(|_| String::from("lỗi"));
    to_jstring(&mut env, &text)
}

#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeReloadFilters<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    files_dir: JString<'local>,
) -> jstring {
    let dir = from_jstring(&mut env, &files_dir);
    let text = catch_unwind(AssertUnwindSafe(|| adblock::reload(&dir)))
        .unwrap_or_else(|_| String::from("lỗi"));
    to_jstring(&mut env, &text)
}

#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeSelfTest<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> jstring {
    let text = catch_unwind(AssertUnwindSafe(|| adblock::self_test()))
        .unwrap_or_else(|_| String::from("lỗi (panic)"));
    to_jstring(&mut env, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_escaped() {
        let items = vec![(
            "Tiêu \"đề\"\n".to_string(),
            "https://a.com/?q=\\x".to_string(),
        )];
        assert_eq!(
            suggestions_json(&items),
            "[{\"t\":\"Tiêu \\\"đề\\\"\\n\",\"u\":\"https://a.com/?q=\\\\x\"}]"
        );
        assert_eq!(suggestions_json(&[]), "[]");
    }
}
