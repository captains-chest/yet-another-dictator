#!/usr/bin/env bash
# Downloads a Whisper model into apps/macos/Model and verifies every file against scripts/model-manifest.txt
# (pinned Hugging Face commits + SHA-256). Idempotent. For the end-to-end test only: the app does
# not bundle the model; it downloads and verifies it on first launch (ADR 0004).
#   apps/macos/scripts/fetch-model.sh [model-id]   (default: the first model in the manifest)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
manifest="$root/scripts/model-manifest.txt"
dest="$root/Model"

sha256() { shasum -a 256 "$1" | awk '{print $1}'; }

model="${1:-$(awk '$1 !~ /^#/ && NF == 5 { print $1; exit }' "$manifest")}"

mkdir -p "$dest"
while read -r id expected _bytes remote path; do
    [[ -z "$id" || "$id" == \#* || "$id" != "$model" ]] && continue
    case "$path" in /* | *..*) echo "refusing unsafe path: $path" >&2; exit 1 ;; esac

    target="$dest/$path"
    if [[ -f "$target" && "$(sha256 "$target")" == "$expected" ]]; then
        continue
    fi

    echo "↓ $path"
    mkdir -p "$(dirname "$target")"
    tmp="$(mktemp "$dest/.download.XXXXXX")"
    trap 'rm -f "$tmp"' EXIT
    curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location --retry 3 \
        --output "$tmp" "https://huggingface.co/$remote"

    actual="$(sha256 "$tmp")"
    if [[ "$actual" != "$expected" ]]; then
        echo "checksum mismatch for $path: expected $expected, got $actual" >&2
        exit 1
    fi
    mv "$tmp" "$target"
    trap - EXIT
done < "$manifest"

# Remove files that are not in the manifest for this model, so a reused Model/ folder stays exact.
expected_paths="$(awk -v m="$model" '$1 == m && NF == 5 { print $5 }' "$manifest")"
while IFS= read -r -d '' file; do
    relative="${file#"$dest"/}"
    if ! grep -Fxq -- "$relative" <<< "$expected_paths"; then
        echo "✗ removing $relative (not in manifest)"
        rm -f "$file"
    fi
done < <(find "$dest" -type f ! -name '.*' -print0)
find "$dest" -mindepth 1 -type d -empty -delete

echo "✓ Model verified in $dest"
