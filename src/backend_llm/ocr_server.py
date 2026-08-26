#!/usr/bin/env python3
"""Start vLLM OCR after registering the image codecs used by Momento."""

import runpy

from image_runtime import register_image_decoders


def main():
    register_image_decoders()
    runpy.run_module("vllm.entrypoints.openai.api_server", run_name="__main__")


if __name__ == "__main__":
    main()
