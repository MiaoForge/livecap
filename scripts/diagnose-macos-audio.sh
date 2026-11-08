#!/bin/sh
set -eu
output_dir=$(mktemp -d)
trap 'rm -rf "$output_dir"' EXIT
xcrun swiftc -swift-version 5 -parse-as-library \
  src-tauri/native/AudioCapture.swift src-tauri/native/AudioCaptureProbe.swift \
  -o "$output_dir/audio-probe"
"$output_dir/audio-probe" "$@"
