#!/usr/bin/env python3
"""Serve bounded descriptor requests through the SGLang OCR batch engine."""

import argparse
import base64
import io
import sys
import time
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ocr.engine import SglangEngine
from image_runtime import (
    ModelHTTPServer,
    create_inference_slots,
    decode_image,
    register_image_decoders,
)
from runtime_http import ImageRuntimeRequestHandler
from runtime_input import read_runtime_input


class OcrRuntime:
    def __init__(self, engine, logit_processor, max_tokens, max_concurrent_jobs):
        if max_tokens <= 0:
            raise ValueError("max_tokens must be positive")
        self.engine = engine
        self.logit_processor = logit_processor
        self.max_tokens = max_tokens
        self.slots = create_inference_slots(max_concurrent_jobs)

    def infer(self, image):
        request_id = uuid.uuid4().hex
        started = time.monotonic()
        with io.BytesIO() as encoded:
            image.save(encoded, format="PNG")
            data = base64.b64encode(encoded.getbuffer()).decode("ascii")
        payload = {
            "model": "Unlimited-OCR-2",
            "rid": request_id,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "image_url",
                            "image_url": {"url": "data:image/png;base64," + data},
                        },
                        {"type": "text", "text": "<PAGE><image>Multi page parsing."},
                    ],
                }
            ],
            "max_tokens": self.max_tokens,
            "temperature": 0,
            "skip_special_tokens": False,
            "images_config": {"crop_mode": True},
            "custom_logit_processor": self.logit_processor,
            "custom_params": {
                "ngram_size": 35,
                "window_size": 128,
                "page_token_id": 128827,
                "eos_token_id": 1,
                "max_page_tokens": 0,
            },
            "stream": False,
        }
        print(
            f"OCR started request_id={request_id} max_tokens={self.max_tokens}",
            flush=True,
        )
        result = self.engine.complete(payload)
        choices = result.get("choices") if isinstance(result, dict) else None
        if (
            not isinstance(choices, list)
            or len(choices) != 1
            or not isinstance(choices[0], dict)
        ):
            raise RuntimeError("SGLang OCR response must contain one result")
        choice = choices[0]
        if choice.get("finish_reason") != "stop":
            raise RuntimeError("OCR output did not finish before its token limit")
        message = choice.get("message")
        text = message.get("content") if isinstance(message, dict) else None
        if not isinstance(text, str):
            raise RuntimeError("OCR output has no text string")
        print(
            f"OCR completed request_id={request_id} seconds={time.monotonic() - started:.3f}",
            flush=True,
        )
        return {"text": text}


class OcrRequestHandler(ImageRuntimeRequestHandler):
    def run_inference(self):
        with self.runtime.slots:
            with read_runtime_input(self, self.input_root) as source:
                image = decode_image(source)
            try:
                return self.runtime.infer(image)
            finally:
                image.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model-path", required=True)
    parser.add_argument("--device", required=True)
    parser.add_argument("--host", required=True)
    parser.add_argument("--port", required=True, type=int)
    parser.add_argument("--max-tokens", required=True, type=int)
    parser.add_argument("--max-concurrent-jobs", required=True, type=int)
    parser.add_argument("--startup-timeout", required=True, type=int)
    parser.add_argument("--request-timeout", required=True, type=int)
    parser.add_argument("--input-root", required=True)
    parser.add_argument("--cache-dir", required=True)
    arguments = parser.parse_args()
    if not 0 < arguments.port < 65535 or not 0 < arguments.max_concurrent_jobs <= 64:
        parser.error("OCR requires a valid port and between 1 and 64 concurrent jobs")
    if (
        min(arguments.max_tokens, arguments.startup_timeout, arguments.request_timeout)
        <= 0
    ):
        parser.error("OCR token limit and timeouts must be positive")

    from sglang.srt.sampling.custom_logit_processor import (
        DeepseekOCRNoRepeatNGramLogitProcessor,
    )

    register_image_decoders()
    engine = SglangEngine(
        arguments.model_path,
        arguments.device,
        arguments.port + 1,
        arguments.max_concurrent_jobs,
        arguments.cache_dir,
        arguments.request_timeout,
    )
    server = None
    try:
        engine.wait_until_ready(arguments.startup_timeout)
        OcrRequestHandler.runtime = OcrRuntime(
            engine,
            DeepseekOCRNoRepeatNGramLogitProcessor.to_str(),
            arguments.max_tokens,
            arguments.max_concurrent_jobs,
        )
        OcrRequestHandler.input_root = arguments.input_root
        server = ModelHTTPServer((arguments.host, arguments.port), OcrRequestHandler)
        server.timeout = 1
        while engine.is_alive():
            server.handle_request()
        raise RuntimeError(f"SGLang engine exited: {engine.process.returncode}")
    except KeyboardInterrupt:
        pass
    finally:
        if server is not None:
            server.server_close()
        engine.stop()


if __name__ == "__main__":
    main()
