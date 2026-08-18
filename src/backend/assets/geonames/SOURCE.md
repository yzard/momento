# GeoNames cities500 snapshot

This directory contains a deterministic, preprocessed snapshot of the GeoNames
`cities500` dataset used for local reverse geocoding.

- Acquired: 2026-08-17 in America/New_York (2026-08-18 UTC)
- GeoNames source snapshot date: 2026-08-18
- Records: 235,408
- Output format: gzip-compressed UTF-8 TSV
- Output SHA-256: `9d43c79540f5dd7b706132972a1d92845189148d5914ef2ce14a179870ffcb69`

Source files:

| File | URL | SHA-256 |
|---|---|---|
| `cities500.zip` | https://download.geonames.org/export/dump/cities500.zip | `9455e6646c09391a3ef2c729b56a79155a512c54eadad728277fbf0dff45b94f` |
| `admin1CodesASCII.txt` | https://download.geonames.org/export/dump/admin1CodesASCII.txt | `590651498043f674accda2b7f46d21286cda0e290b02f8561c5005eee9a5448c` |
| `countryInfo.txt` | https://download.geonames.org/export/dump/countryInfo.txt | `93bafc525813f22e4711ff9ed6d626343094ce48c26388dc7c49189b3d7d5512` |

Momento adapted and preprocessed the source files into the bundled dataset.
The generated columns are `latitude`, `longitude`, `city`, `state`, and
`country`. State names come from `admin1CodesASCII.txt`; country names come
from `countryInfo.txt`.

GeoNames data is licensed under the Creative Commons Attribution 4.0 License:
https://creativecommons.org/licenses/by/4.0/

GeoNames attribution: https://www.geonames.org/
