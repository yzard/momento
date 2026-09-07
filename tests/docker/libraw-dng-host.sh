#!/bin/sh
set -eu
# Run in the API runtime test stage to exercise ImageMagick's LibRaw C interface.
fixtures="$1"
output_dir="$2"
mkdir -p "$output_dir"
for name in 01_jxl_linear_raw_integer 02_jxl_linear_raw_float 03_jxl_bayer_raw_integer; do
    magick "$fixtures/$name.dng[0]" -auto-orient -quality 90 "$output_dir/$name.jpg"
    magick identify "$output_dir/$name.jpg"
    test -s "$output_dir/$name.jpg"
done
