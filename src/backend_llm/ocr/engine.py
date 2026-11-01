"""Supervise the image-owned SGLang engine and its loopback HTTP transport."""

import json
import os
import subprocess
import sys
import sysconfig
import time
import urllib.error
import urllib.request
from pathlib import Path

MAX_RESPONSE_BYTES = 16 * 1024 * 1024


class EngineUnavailable(Exception):
    """Close the request when engine transport is lost so durable work can retry."""


def engine_command(model_path, device, port, max_concurrent_jobs):
    if device == "cuda":
        gpu_id = 0
    elif device.startswith("cuda:") and device[5:].isdigit():
        gpu_id = int(device[5:])
    else:
        raise ValueError("Unlimited-OCR-2 requires a CUDA device")
    return [
        sys.executable,
        "-m",
        "sglang.launch_server",
        "--model-path",
        model_path,
        "--served-model-name",
        "Unlimited-OCR-2",
        "--chat-template",
        "unlimited-ocr",
        "--trust-remote-code",
        "--tokenizer-backend",
        "fastokens",
        "--tokenizer-worker-num",
        "2",
        "--base-gpu-id",
        str(gpu_id),
        "--mem-fraction-static",
        "0.65",
        "--context-length",
        "131072",
        "--max-running-requests",
        str(max_concurrent_jobs),
        "--decode-log-interval",
        "256",
        "--max-prefill-tokens",
        "2048",
        "--enable-custom-logit-processor",
        "--disable-radix-cache",
        "--mm-preprocess-cache-size-mb",
        "0",
        "--cuda-graph-backend-decode",
        "full",
        "--cuda-graph-max-bs-decode",
        str(max_concurrent_jobs),
        "--cuda-graph-backend-prefill",
        "full",
        "--cuda-graph-max-bs-prefill",
        "2048",
        "--cuda-graph-bs-prefill",
        "1024",
        "2048",
        "--cuda-graph-config",
        json.dumps(
            {
                "prefill": {
                    "backend": "full",
                    "full_prefill_max_req": max_concurrent_jobs,
                }
            }
        ),
        "--host",
        "127.0.0.1",
        "--port",
        str(port),
    ]


class SglangEngine:
    def __init__(
        self, model_path, device, port, max_concurrent_jobs, cache_dir, request_timeout
    ):
        self.base_url = f"http://127.0.0.1:{port}"
        self.request_timeout = request_timeout
        cache = Path(cache_dir)
        modules = cache / "huggingface" / "modules"
        modules.mkdir(parents=True, exist_ok=True)
        environment = os.environ.copy()
        cuda_home = Path(sysconfig.get_path("purelib")) / "nvidia" / "cu13"
        if not os.access(cuda_home / "bin" / "nvcc", os.X_OK):
            raise RuntimeError("SGLang requires the bundled CUDA 13 compiler")
        environment.update(
            {
                "HF_MODULES_CACHE": str(modules),
                "XDG_CACHE_HOME": str(cache),
                "TRITON_CACHE_DIR": str(cache / "triton"),
                "CUDA_CACHE_PATH": str(cache / "cuda"),
                "CUDA_HOME": str(cuda_home),
                "CUDA_PATH": str(cuda_home),
                "PATH": str(cuda_home / "bin")
                + os.pathsep
                + str(Path(sys.executable).parent)
                + os.pathsep
                + environment.get("PATH", ""),
                "SGLANG_UOCR_EANABLE_DOC_LIMIT": "0",
                "SGLANG_UOCR_MAX_PAGE_TOKENS": "0",
                "SGLANG_OPT_UOCR_SAM_REL_POS_BIAS_MB": "64",
                "SGLANG_UOCR_SHM_WRAP_MODEL_DATA": "1",
                "SGLANG_UOCR_SHM_WRAP_MIN_MB": "1",
                "SGLANG_VLM_CACHE_SIZE_MB": "0",
                "SGLANG_UOCR_NGRAM_OUTPUT_ONLY": "1",
                "PYTORCH_CUDA_ALLOC_CONF": "expandable_segments:True",
            }
        )
        # Inherit the managed runtime's process group so Rust can stop every worker.
        self.process = subprocess.Popen(
            engine_command(model_path, device, port, max_concurrent_jobs),
            env=environment,
        )
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def is_alive(self):
        return self.process.poll() is None

    def wait_until_ready(self, timeout):
        deadline = time.monotonic() + timeout
        while self.is_alive():
            try:
                with self.opener.open(self.base_url + "/health", timeout=2) as response:
                    if response.status == 200:
                        return
            except (OSError, urllib.error.URLError):
                pass
            if time.monotonic() >= deadline:
                raise RuntimeError("SGLang startup exceeded its configured timeout")
            time.sleep(0.2)
        raise RuntimeError(f"SGLang exited during startup: {self.process.returncode}")

    def complete(self, payload):
        request = urllib.request.Request(
            self.base_url + "/v1/chat/completions",
            data=json.dumps(payload).encode("utf-8"),
            headers={"Content-Type": "application/json"},
        )
        try:
            with self.opener.open(request, timeout=self.request_timeout) as response:
                body = response.read(MAX_RESPONSE_BYTES + 1)
        except urllib.error.HTTPError as error:
            detail = error.read(2048).decode("utf-8", errors="replace")
            raise RuntimeError(
                f"SGLang returned HTTP {error.code}: {detail}"
            ) from error
        except (OSError, urllib.error.URLError) as error:
            raise EngineUnavailable(f"SGLang transport failed: {error}") from error
        if len(body) > MAX_RESPONSE_BYTES:
            raise RuntimeError("SGLang response exceeds the result size limit")
        try:
            return json.loads(body)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise RuntimeError("SGLang returned invalid JSON") from error

    def stop(self):
        if self.process.poll() is not None:
            return
        self.process.terminate()
        try:
            self.process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
