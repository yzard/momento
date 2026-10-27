#!/usr/bin/env python3
"""Check that an exported OCI image contains every referenced blob."""
import hashlib
import json
import re
import sys
from pathlib import Path


def validate(root: Path) -> None:
    visited = set()

    def descriptor(item: dict) -> None:
        digest = item["digest"]
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", digest):
            raise ValueError("Unsupported OCI blob digest")
        blob = root / "blobs" / "sha256" / digest.split(":", 1)[1]
        if blob.stat().st_size != item["size"]:
            raise ValueError("OCI blob size differs from the manifest")
        if digest in visited:
            return
        visited.add(digest)
        media_type = item.get("mediaType", "")
        if "manifest" in media_type or "index" in media_type:
            payload = blob.read_bytes()
            if hashlib.sha256(payload).hexdigest() != digest.split(":", 1)[1]:
                raise ValueError("OCI manifest digest differs from its contents")
            manifest(json.loads(payload))

    def manifest(value: dict) -> None:
        if value.get("schemaVersion") != 2:
            raise ValueError("Unsupported OCI manifest schema")
        for item in value.get("manifests", []):
            descriptor(item)
        if "config" in value:
            descriptor(value["config"])
        for item in value.get("layers", []):
            descriptor(item)

    index = json.loads((root / "index.json").read_text())
    if not index.get("manifests"):
        raise ValueError("OCI index contains no images")
    manifest(index)


if __name__ == "__main__":
    try:
        validate(Path(sys.argv[1]))
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"Invalid reusable OCI image: {error}", file=sys.stderr)
        sys.exit(1)
