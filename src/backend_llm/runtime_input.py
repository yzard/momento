"""Validated access to inputs shared by the llm-service scheduler."""

import hashlib
import json
import os
import stat


MAX_DESCRIPTOR_BYTES = 64 * 1024
MAX_INPUT_BYTES = 32 * 1024 * 1024 * 1024
DESCRIPTOR_FIELDS = {"jobId", "sequence", "byteSize", "contentHash", "mimeType"}


def read_runtime_input(handler, input_root):
    if handler.headers.get("Content-Type") != "application/json":
        raise ValueError("Content-Type must be application/json")
    content_length = int(handler.headers.get("Content-Length", "0"))
    if content_length <= 0 or content_length > MAX_DESCRIPTOR_BYTES:
        raise ValueError(
            f"Content-Length must be between 1 and {MAX_DESCRIPTOR_BYTES}"
        )
    descriptor = json.loads(handler.rfile.read(content_length))
    if not isinstance(descriptor, dict) or set(descriptor) != DESCRIPTOR_FIELDS:
        raise ValueError("runtime input descriptor has invalid fields")

    job_id = descriptor["jobId"]
    sequence = descriptor["sequence"]
    byte_size = descriptor["byteSize"]
    content_hash = descriptor["contentHash"]
    mime_type = descriptor["mimeType"]
    if (
        not isinstance(job_id, str)
        or not job_id
        or not all(character in "0123456789abcdefABCDEF" for character in job_id)
    ):
        raise ValueError("jobId must be non-empty hexadecimal text")
    if isinstance(sequence, bool) or not isinstance(sequence, int) or sequence < 0:
        raise ValueError("sequence must be a non-negative integer")
    if (
        isinstance(byte_size, bool)
        or not isinstance(byte_size, int)
        or not 0 < byte_size <= MAX_INPUT_BYTES
    ):
        raise ValueError(f"byteSize must be between 1 and {MAX_INPUT_BYTES}")
    if (
        not isinstance(content_hash, str)
        or len(content_hash) != 64
        or not all(character in "0123456789abcdef" for character in content_hash)
    ):
        raise ValueError("contentHash must be 64 lowercase hexadecimal characters")
    if not isinstance(mime_type, str) or not mime_type.startswith("image/"):
        raise ValueError("mimeType must be an image MIME type")

    if not hasattr(os, "O_NOFOLLOW"):
        raise RuntimeError("runtime input access requires O_NOFOLLOW support")
    root_flags = os.O_RDONLY | os.O_DIRECTORY
    no_follow = os.O_NOFOLLOW
    root_fd = os.open(input_root, root_flags | no_follow)
    try:
        job_fd = os.open(job_id, root_flags | no_follow, dir_fd=root_fd)
        try:
            input_fd = os.open(
                f"input-{sequence}", os.O_RDONLY | no_follow, dir_fd=job_fd
            )
        finally:
            os.close(job_fd)
    finally:
        os.close(root_fd)

    try:
        metadata = os.fstat(input_fd)
        if not stat.S_ISREG(metadata.st_mode):
            raise ValueError("runtime input is not a regular file")
        if metadata.st_size != byte_size:
            raise ValueError("runtime input size does not match descriptor")
        input_file = os.fdopen(input_fd, "rb")
        input_fd = -1
    finally:
        if input_fd >= 0:
            os.close(input_fd)

    try:
        hasher = hashlib.sha256()
        while chunk := input_file.read(1024 * 1024):
            hasher.update(chunk)
        if hasher.hexdigest() != content_hash:
            raise ValueError("runtime input hash does not match descriptor")
        input_file.seek(0)
        return input_file
    except (OSError, ValueError):
        input_file.close()
        raise
