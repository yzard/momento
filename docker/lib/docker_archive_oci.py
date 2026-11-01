#!/usr/bin/env python3
"""Convert one Docker save archive into a temporary OCI build context."""
import gzip
import hashlib
import json
import sys
import tarfile
from pathlib import Path


def convert(archive_path: Path, destination: Path) -> None:
    blobs = destination / "blobs" / "sha256"
    blobs.mkdir(parents=True)

    def store(data: bytes, media_type: str) -> dict:
        digest = hashlib.sha256(data).hexdigest()
        (blobs / digest).write_bytes(data)
        return {"mediaType": media_type, "digest": f"sha256:{digest}", "size": len(data)}

    with tarfile.open(archive_path, "r:*") as archive:
        def metadata(name: str) -> bytes:
            member = archive.getmember(name)
            if not member.isfile() or member.size > 16 * 1024 * 1024:
                raise ValueError("Invalid Docker archive metadata")
            with archive.extractfile(member) as source:
                return source.read()

        images = json.loads(metadata("manifest.json"))
        if len(images) != 1:
            raise ValueError("Expected exactly one Docker image")
        image = images[0]
        config_bytes = metadata(image["Config"])
        config = json.loads(config_bytes)
        expected_layers = config["rootfs"]["diff_ids"]
        if len(expected_layers) != len(image["Layers"]):
            raise ValueError("Docker layer count differs from its configuration")
        layers = []
        for index, (name, expected) in enumerate(zip(image["Layers"], expected_layers)):
            pending = blobs / f"pending-{index}"
            digest = hashlib.sha256()
            size = 0
            member = archive.getmember(name)
            if not member.isfile():
                raise ValueError("Docker layer is not a regular file")
            with archive.extractfile(member) as raw:
                prefix = raw.read(2)
                raw.seek(0)
                source = gzip.GzipFile(fileobj=raw) if prefix == b"\x1f\x8b" else raw
                with pending.open("wb") as output:
                    while chunk := source.read(1024 * 1024):
                        output.write(chunk)
                        digest.update(chunk)
                        size += len(chunk)
            actual = f"sha256:{digest.hexdigest()}"
            if actual != expected:
                raise ValueError("Docker layer digest differs from its configuration")
            pending.replace(blobs / digest.hexdigest())
            layers.append({"mediaType": "application/vnd.oci.image.layer.v1.tar", "digest": actual, "size": size})
        config_descriptor = store(config_bytes, "application/vnd.oci.image.config.v1+json")
        manifest = store(json.dumps({"schemaVersion": 2, "config": config_descriptor, "layers": layers}).encode(),
                         "application/vnd.oci.image.manifest.v1+json")
        (destination / "index.json").write_text(json.dumps({"schemaVersion": 2, "manifests": [manifest]}))
        (destination / "oci-layout").write_text('{"imageLayoutVersion":"1.0.0"}\n')


if __name__ == "__main__":
    try:
        convert(Path(sys.argv[1]), Path(sys.argv[2]))
    except (OSError, ValueError, KeyError, TypeError, tarfile.TarError) as error:
        print(f"Docker image export failed: {error}", file=sys.stderr)
        sys.exit(1)
