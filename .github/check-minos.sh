#!/usr/bin/env bash
# Fails if any Mach-O file under the given paths claims a macOS minimum newer than 10.15.
#
# This repository only ships for macOS 10.15 Catalina, and a single library built against 11.0 keeps the app from starting
# there. GitHub has no 10.15 runner, so the build runs on a newer Intel macOS and `vtool` is what proves the deployment
# target stuck (to the binaries and to the third-party libraries the bundle carries).
#
# Usage: bash .github/check-minos.sh target/release/openfader target/release/*.dylib
set -u

limit=10.15

# `ver_le A B`: A is not newer than B, comparing dot-separated numbers (`10.9` is older than `10.15`).
ver_le() {
    awk -v a="$1" -v b="$2" 'BEGIN {
        n = split(a, x, "."); m = split(b, y, ".")
        for (i = 1; i <= (n > m ? n : m); i++) {
            if (x[i] + 0 < y[i] + 0) exit 0
            if (x[i] + 0 > y[i] + 0) exit 1
        }
        exit 0
    }'
}

fail=0
seen=0
while IFS= read -r -d '' f; do
    case "$(file -b "$f")" in
    *Mach-O*) ;;
    *) continue ;;
    esac
    seen=$((seen + 1))
    # `vtool` reads the LC_BUILD_VERSION command (Xcode 9 and later). Older binaries (the static ffmpeg, built with an older
    # toolchain) carry LC_VERSION_MIN_MACOSX instead, which only `otool -l` prints.
    min=$(vtool -show-build "$f" 2>/dev/null | awk '/minos/ {print $2; exit}')
    if [ -z "$min" ]; then
        min=$(otool -l "$f" 2>/dev/null |
            awk '/LC_VERSION_MIN_MACOSX/ {found = 1} found && $1 == "version" {print $2; exit}')
    fi
    if [ -z "$min" ]; then
        echo "FAIL $f: no minimum macOS version in its load commands" >&2
        fail=1
    elif ver_le "$min" "$limit"; then
        echo "ok   $f: minos $min"
    else
        echo "FAIL $f: minos $min, newer than $limit" >&2
        fail=1
    fi
done < <(find "$@" -type f -print0 2>/dev/null)

if [ "$seen" -eq 0 ]; then
    echo "FAIL no Mach-O file among: $*" >&2
    exit 1
fi
exit $fail
