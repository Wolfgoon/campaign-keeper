#!/usr/bin/env bash
# Builds the Android app: "Campaign Keeper <version> (Android).apk" in ../installers.
# Run it from Git Bash:  ./build-android.sh
#
# Needs Android Studio (for the SDK and Java), the Android NDK, and Rust's Android target:
#   rustup target add aarch64-linux-android
# The APK is signed with the key named in src-tauri/gen/android/keystore.properties.
set -euo pipefail
cd "$(dirname "$0")"

export ANDROID_HOME="${ANDROID_HOME:-$LOCALAPPDATA/Android/Sdk}"
export NDK_HOME="${NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* | tail -1)}"
export JAVA_HOME="${JAVA_HOME:-/c/Program Files/Android/Android Studio/jbr}"

version=$(node -p "require('./package.json').version")
android=src-tauri/gen/android
log=$(mktemp)

# Phones made since about 2017 are all arm64, so that's the one build.
if ! npx tauri android build --apk --target aarch64 2>&1 | tee "$log"; then
  # Without Windows Developer Mode, Tauri can't make the symlink that hands the compiled app to
  # Gradle. Everything else worked, so copy the file there and let Gradle finish.
  grep -q "symbolic link" "$log" || { echo "The Android build failed (see above)."; exit 1; }
  echo "Copying the compiled app in place of the symlink, and finishing the build…"
  mkdir -p "$android/app/src/main/jniLibs/arm64-v8a"
  cp src-tauri/target/aarch64-linux-android/release/libcampaign_keeper_lib.so "$android/app/src/main/jniLibs/arm64-v8a/"
  (cd "$android" && ./gradlew assembleArm64Release -x rustBuildArm64Release --console=plain)
fi

apk=$(ls -t "$android"/app/build/outputs/apk/arm64/release/*.apk | head -1)
case "$apk" in *unsigned*) echo "Warning: no signing key is set up, so this APK won't install. See keystore.properties." ;; esac
mkdir -p ../installers
cp "$apk" "../installers/Campaign Keeper $version (Android).apk"
echo "Built: installers/Campaign Keeper $version (Android).apk"
