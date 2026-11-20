#!/usr/bin/env bash
# Dymith CI: dựng lõi Rust + APK Android, kiểm tra căn trang 16 KB, phát hành lên GitHub Release.
# Chỉ dùng công cụ cài sẵn trên runner ubuntu-24.04 (không phụ thuộc action của bên thứ ba).
# Lưu ý: tránh "| head" và "grep -q" trong pipeline vì set -o pipefail coi SIGPIPE là lỗi.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
mkdir -p out logs

: "${GITHUB_SHA:=local0000000}"
: "${GITHUB_RUN_NUMBER:=1}"
BRANCH="${GITHUB_REF_NAME:-main}"
APK_NAME="dymith-phase0-arm64.apk"
GRADLE_VERSION="8.9"
RELEASE_TAG="nightly"

banner() { printf '\n==== %s ====\n' "$*"; }

# ---------------------------------------------------------------- 0. Xóa tệp cũ theo ci/delete.txt
if [ -f ci/delete.txt ]; then
  banner "0/8 Xóa tệp đã gỡ (ci/delete.txt)"
  while IFS= read -r f; do
    case "$f" in ''|'#'*) continue ;; esac
    echo "xóa: $f"
    git rm -q -f --ignore-unmatch -- "$f" || true
    rm -f -- "$f"
  done < ci/delete.txt
  git rm -q -f --ignore-unmatch -- ci/delete.txt || true
  rm -f ci/delete.txt
  git add -A
  git commit -q -m "ci: xóa tệp đã gỡ" || true
  git pull --rebase -q origin "$BRANCH" || true
  git push -q origin "HEAD:${BRANCH}" || echo "CẢNH BÁO: không push được commit xóa tệp"
fi

# ---------------------------------------------------------------- 1. Java + Rust
banner "1/8 Môi trường"
# Gradle 8.9 + AGP 8.7 cần JDK 17 hoặc 21 (không chạy được trên JDK 25).
if [ -n "${JAVA_HOME_17_X64:-}" ] && [ -x "${JAVA_HOME_17_X64}/bin/java" ]; then
  export JAVA_HOME="$JAVA_HOME_17_X64"
elif [ -n "${JAVA_HOME_21_X64:-}" ] && [ -x "${JAVA_HOME_21_X64}/bin/java" ]; then
  export JAVA_HOME="$JAVA_HOME_21_X64"
else
  echo "Runner không có JDK 17/21 như mong đợi"
  exit 1
fi
export PATH="$JAVA_HOME/bin:$PATH"
echo "JAVA_HOME=$JAVA_HOME"
java -version 2>&1 | sed -n '1,2p'
rustc --version
cargo --version

# ---------------------------------------------------------------- 2. Android SDK + NDK
banner "2/8 Android SDK + NDK"
export ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-/usr/local/lib/android/sdk}}"
export ANDROID_SDK_ROOT="$ANDROID_HOME"
SDKM="$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager"
if [ ! -x "$SDKM" ]; then SDKM="$(command -v sdkmanager || true)"; fi
if [ -z "$SDKM" ]; then echo "Không tìm thấy sdkmanager"; exit 1; fi

yes | "$SDKM" --licenses > /dev/null 2>&1 || true
"$SDKM" --install "platforms;android-35" "build-tools;35.0.0" > /dev/null

# Cần NDK r28 trở lên: mặc định căn ELF 16 KB. Ưu tiên bản cài sẵn, không có thì cài r28c.
NDK=""
for d in "$ANDROID_HOME"/ndk/*; do
  [ -d "$d" ] || continue
  major="$(basename "$d")"
  major="${major%%.*}"
  case "$major" in ''|*[!0-9]*) continue ;; esac
  if [ "$major" -ge 28 ]; then NDK="$d"; fi
done
if [ -z "$NDK" ]; then
  echo "Runner chưa có NDK r28+, đang cài ndk;28.2.13676358"
  "$SDKM" --install "ndk;28.2.13676358" > /dev/null
  NDK="$ANDROID_HOME/ndk/28.2.13676358"
fi
echo "NDK: $NDK"
TC="$NDK/toolchains/llvm/prebuilt/linux-x86_64"
CLANG="$TC/bin/aarch64-linux-android26-clang"
if [ ! -x "$CLANG" ]; then
  echo "Không thấy $CLANG"
  ls "$TC/bin" | sed -n '1,40p'
  exit 1
fi

# ---------------------------------------------------------------- 3. Rust: unit test (không chặn build)
banner "3/8 Rust: thêm target + chạy unit test lõi mới"
rustup target add aarch64-linux-android
TEST_SUMMARY="không chạy"
if cargo test --manifest-path core/Cargo.toml > logs/rust-test.log 2>&1; then
  TEST_SUMMARY="$(grep -E '^test result' logs/rust-test.log | tr '\n' ' ' || true)"
  if [ -z "$TEST_SUMMARY" ]; then TEST_SUMMARY="đạt"; fi
else
  TEST_SUMMARY="THẤT BẠI (không chặn build)"
  echo "CẢNH BÁO: unit test Rust thất bại, 40 dòng cuối:"
  tail -n 40 logs/rust-test.log
fi
echo "Unit test Rust: $TEST_SUMMARY"

# ---------------------------------------------------------------- 4. Rust: biên dịch (có lõi dự phòng)
banner "4/8 Rust: biên dịch lõi cho arm64 (release)"
export CC_aarch64_linux_android="$CLANG"
export AR_aarch64_linux_android="$TC/bin/llvm-ar"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$CLANG"
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-z,max-page-size=16384"
export DYMITH_GIT_SHA="${GITHUB_SHA:0:7}"
mkdir -p app/src/main/jniLibs/arm64-v8a
SO="app/src/main/jniLibs/arm64-v8a/libdymith_core.so"

CORE_USED="v0.0.2 (đầy đủ)"
ERR_EXCERPT=""
if cargo build --release --target aarch64-linux-android -p dymith-core > logs/rust-new.log 2>&1; then
  tail -n 3 logs/rust-new.log
  cp target/aarch64-linux-android/release/libdymith_core.so "$SO"
else
  echo "!!! Lõi mới KHÔNG biên dịch được. 80 dòng cuối của nhật ký:"
  tail -n 80 logs/rust-new.log
  ERR_EXCERPT="$(tail -n 40 logs/rust-new.log)"
  CORE_USED="DỰ PHÒNG v0.0.1 (lõi mới lỗi biên dịch)"
  echo "Chuyển sang lõi dự phòng core-lite (v0.0.1) để vẫn có APK dùng được"
  cargo build --release --target aarch64-linux-android \
    --manifest-path core-lite/Cargo.toml --target-dir core-lite/target
  cp core-lite/target/aarch64-linux-android/release/libdymith_core.so "$SO"
fi
echo "Lõi đang dùng: $CORE_USED"
ls -l "$SO"

banner "5/8 Kiểm tra căn ELF 16 KB"
READELF="$TC/bin/llvm-readelf"
"$READELF" -lW "$SO" | grep ' LOAD ' | tee logs/elf-load.txt
bad=0
while read -r line; do
  align="${line##* }"
  if [ $((align)) -lt 16384 ]; then bad=1; fi
done < logs/elf-load.txt
if [ "$bad" -ne 0 ]; then
  echo "LỖI: có segment LOAD căn nhỏ hơn 16 KB"
  exit 1
fi
echo "ELF căn 16 KB: OK"

# ---------------------------------------------------------------- 6. Gradle
banner "6/8 Khóa ký debug cố định + Gradle assembleRelease"
mkdir -p app/signing
if [ ! -f app/signing/dymith-debug.p12 ]; then
  keytool -genkeypair -storetype PKCS12 -keystore app/signing/dymith-debug.p12 \
    -storepass android -keypass android -alias dymith -keyalg RSA -keysize 2048 \
    -validity 36500 -dname "CN=Dymith Debug, O=Dymith, C=VN"
  echo "Đã tạo khóa debug mới (sẽ được lưu vào repo)"
fi

GRADLE_DIR="$HOME/.dymith-gradle/gradle-$GRADLE_VERSION"
if [ ! -x "$GRADLE_DIR/bin/gradle" ]; then
  mkdir -p "$HOME/.dymith-gradle"
  curl -fsSL --retry 3 "https://services.gradle.org/distributions/gradle-${GRADLE_VERSION}-bin.zip" -o /tmp/gradle.zip
  unzip -q /tmp/gradle.zip -d "$HOME/.dymith-gradle"
fi
export PATH="$GRADLE_DIR/bin:$PATH"
gradle --version | sed -n '1,12p'
export DYMITH_VERSION_CODE="$GITHUB_RUN_NUMBER"
gradle --no-daemon --console=plain --stacktrace :app:assembleRelease

# ---------------------------------------------------------------- 7. Kiểm tra APK + báo cáo
banner "7/8 Kiểm tra APK và lập báo cáo"
APK="app/build/outputs/apk/release/app-release.apk"
if [ ! -f "$APK" ]; then echo "Không thấy $APK"; exit 1; fi
cp "$APK" "out/$APK_NAME"
BT="$ANDROID_HOME/build-tools/35.0.0"

ALIGN="LỖI"
if "$BT/zipalign" -c -P 16 4 "out/$APK_NAME"; then ALIGN="OK"; fi
SIGN="$("$BT/apksigner" verify --verbose "out/$APK_NAME" 2>&1 | sed -n '1,6p')" || SIGN="không xác minh được"
BADGE="$("$BT/aapt2" dump badging "out/$APK_NAME" 2>/dev/null | sed -n '/^package:/p;/^sdkVersion:/p;/^targetSdkVersion:/p;/^native-code:/p')" || BADGE=""
APK_SIZE="$(du -h "out/$APK_NAME" | cut -f1)"
SO_SIZE="$(du -h "$SO" | cut -f1)"
HAS_START="KHÔNG CÓ"
if unzip -l "out/$APK_NAME" | grep 'assets/start.html' > /dev/null; then HAS_START="có"; fi

{
  echo "## Dymith Phase 0.2, bản dựng #${GITHUB_RUN_NUMBER}"
  echo
  echo "- Commit: \`${GITHUB_SHA:0:7}\`"
  echo "- Lõi Rust đang dùng: **${CORE_USED}**"
  echo "- Unit test Rust: ${TEST_SUMMARY}"
  echo "- APK arm64: **${APK_SIZE}**, lõi Rust (.so): **${SO_SIZE}**"
  echo "- Căn trang 16 KB (zipalign -P 16): **${ALIGN}**"
  echo "- Trang chủ start.html trong APK: ${HAS_START}"
  echo "- NDK: \`$(basename "$NDK")\`, $(rustc --version)"
  echo
  echo '```'
  echo "$BADGE"
  echo "$SIGN"
  echo '```'
  if [ -n "$ERR_EXCERPT" ]; then
    echo
    echo "### Lỗi biên dịch lõi mới (40 dòng cuối), hãy gửi khối này cho Claude"
    echo '```'
    printf '%s\n' "$ERR_EXCERPT"
    echo '```'
  fi
  echo
  echo "Cài đặt: tải \`${APK_NAME}\` ở mục Assets bên dưới, mở file và cho phép cài từ nguồn này."
} > out/report.md
cat out/report.md

# ---------------------------------------------------------------- 8. Lưu khóa/lockfile + phát hành
banner "8/8 Lưu Cargo.lock và khóa debug vào repo, rồi phát hành"
git add -f Cargo.lock app/signing/dymith-debug.p12 || true
if ! git diff --cached --quiet; then
  git commit -q -m "ci: lưu Cargo.lock và khóa ký debug"
  git pull --rebase -q origin "$BRANCH" || true
  git push -q origin "HEAD:${BRANCH}" || echo "CẢNH BÁO: không push được commit lưu khóa"
fi

if command -v gh > /dev/null 2>&1 && [ -n "${GH_TOKEN:-}" ]; then
  REPO="${GITHUB_REPOSITORY:-}"
  gh release delete "$RELEASE_TAG" --cleanup-tag --yes --repo "$REPO" > /dev/null 2>&1 || true
  if gh release create "$RELEASE_TAG" "out/$APK_NAME" out/report.md --repo "$REPO" \
      --target "$(git rev-parse HEAD)" --prerelease \
      --title "Dymith Phase 0.2, bản dựng #${GITHUB_RUN_NUMBER}" --notes-file out/report.md; then
    echo "Đã phát hành: https://github.com/${REPO}/releases/tag/${RELEASE_TAG}"
  else
    echo "CẢNH BÁO: không tạo được Release. APK vẫn có ở mục Artifacts của lần chạy này."
  fi
else
  echo "Bỏ qua phát hành (thiếu gh hoặc GH_TOKEN)."
fi
echo "XONG"
