//! Dymith Core, Phase 0.
//!
//! Lõi Rust của trình duyệt. Ở Phase 0 chỉ có ba chức năng để kiểm chứng đường ống
//! Rust <-> Kotlin: thông tin phiên bản, phân giải ô địa chỉ và bộ chặn quảng cáo mẫu.

mod blocklist;
mod omnibox;

use std::panic::{catch_unwind, AssertUnwindSafe};

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jstring, JNI_FALSE, JNI_TRUE};
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

#[no_mangle]
pub extern "system" fn Java_dev_dymith_browser_Core_nativeShouldBlock<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    url: JString<'local>,
) -> jboolean {
    let url = from_jstring(&mut env, &url);
    let blocked = catch_unwind(AssertUnwindSafe(|| blocklist::should_block(&url))).unwrap_or(false);
    if blocked {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}
