#!/bin/sh
set -eu
# Run inside a decoder builder: use the checksum-pinned SDK's public fixtures,
# never private photographs. Non-empty TIFF proves a real decode, not registration.
decoder="$1"
fixtures="$2"
output_dir="$3"
mkdir -p "$output_dir"
for name in 01_jxl_linear_raw_integer 02_jxl_linear_raw_float 03_jxl_bayer_raw_integer; do
    "$decoder" -dngsdk -w +M -o 1 -q 3 -T -Z "$output_dir/$name.tiff" "$fixtures/$name.dng"
    test -s "$output_dir/$name.tiff"
done
