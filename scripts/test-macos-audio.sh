#!/bin/sh
set -eu
output_dir=$(mktemp -d)
trap 'rm -rf "$output_dir"' EXIT
xcrun swiftc -swift-version 5 -parse-as-library \
  src-tauri/native/AudioCapture.swift src-tauri/native/AudioCaptureTests.swift \
  -o "$output_dir/audio-tests"
"$output_dir/audio-tests"
