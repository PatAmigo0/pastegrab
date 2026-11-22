#!/usr/bin/env bash
# virustotal.sh FILE...: uploads each file VirusTotal hasn't seen, waits for its scan, and prints
# the engines that flag it. VirusTotal shares uploads with antivirus vendors; send release files only.
# needs: curl jq sha256sum stat
set -euo pipefail
key=${VT_API_KEY:?set VT_API_KEY to a VirusTotal API key}
api=https://www.virustotal.com/api/v3
get() { curl -fsS -H "x-apikey: $key" "$@"; }

for f in "$@"; do
    sha=$(sha256sum "$f" | cut -d' ' -f1)
    if ! get -o /dev/null "$api/files/$sha" 2>/dev/null; then
        url=$api/files
        # Files over 32 MB go to a one-time upload address.
        if [ "$(stat -c %s "$f")" -gt 33554432 ]; then url=$(get "$api/files/upload_url" | jq -r .data); fi
        analysis=$(get -F "file=@$f" "$url" | jq -r .data.id)
        until [ "$(get "$api/analyses/$analysis" | jq -r .data.attributes.status)" = completed ]; do
            sleep 20
        done
    fi
    report=$(get "$api/files/$sha")
    echo "$(basename "$f") $sha: $(jq -r '.data.attributes.last_analysis_stats | "\(.malicious) malicious, \(.suspicious) suspicious, \(.undetected) undetected"' <<<"$report")"
    jq -r '.data.attributes.last_analysis_results[] | select(.category == "malicious" or .category == "suspicious") | "  \(.engine_name): \(.result)"' <<<"$report"
done
