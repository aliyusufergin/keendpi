#!/bin/sh
# Build an opkg feed directory from .ipk files: tools/mkfeed.sh OUTDIR IPK...
# OUTDIR gets the packages plus Packages and Packages.gz (what `src/gz` lines point at).
set -e
OUT=$1; shift
[ -n "$OUT" ] && [ $# -gt 0 ] || { echo "usage: $0 OUTDIR IPK..."; exit 2; }
mkdir -p "$OUT"
: > "$OUT/Packages"
for f in "$@"; do
  b=$(basename "$f")
  cp "$f" "$OUT/$b"
  tar -xzOf "$f" ./control.tar.gz | tar -xzOf - ./control | grep -v '^$' >> "$OUT/Packages"
  {
    echo "Filename: $b"
    echo "Size: $(wc -c < "$f" | tr -d ' ')"
    echo "SHA256sum: $(sha256sum "$f" | cut -d' ' -f1)"
    echo
  } >> "$OUT/Packages"
done
gzip -9nc "$OUT/Packages" > "$OUT/Packages.gz"
echo "$OUT: $# package(s)"
