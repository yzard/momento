from pathlib import Path
from typing import Optional

import yaml
from pydantic import BaseModel, Field

from momento_api.constants import (
    CONFIG_PATH,
    DEFAULT_THUMBNAIL_QUALITY,
    DEFAULT_THUMBNAIL_SIZE,
    DEFAULT_VIDEO_FRAME_QUALITY,
)


class ServerConfig(BaseModel):
    host: str = "0.0.0.0"
    port: int = 8000
    debug: bool = False


class SecurityConfig(BaseModel):
    secret_key: str = Field(default="change-me-in-production-use-openssl-rand-hex-32")
    algorithm: str = "HS256"
    access_token_expire_minutes: int = 30
    refresh_token_expire_days: int = 7


class AdminConfig(BaseModel):
    username: str = "admin"
    password: str = "admin"


class WebDAVConfig(BaseModel):
    enabled: bool = False
    hostname: str = ""
    username: str = ""
    password: str = ""
    remote_path: str = "/"


class ThumbnailConfig(BaseModel):
    max_size: int = DEFAULT_THUMBNAIL_SIZE
    quality: int = DEFAULT_THUMBNAIL_QUALITY
    video_frame_quality: int = DEFAULT_VIDEO_FRAME_QUALITY


class ReverseGeocodingConfig(BaseModel):
    enabled: bool = True
    base_url: str = "https://nominatim.openstreetmap.org/reverse"
    user_agent: str = "Momento/1.0 (self-hosted)"
    timeout_seconds: int = 10
    rate_limit_seconds: float = 1.0


class Config(BaseModel):
    server: ServerConfig = Field(default_factory=ServerConfig)
    security: SecurityConfig = Field(default_factory=SecurityConfig)
    admin: AdminConfig = Field(default_factory=AdminConfig)
    webdav: WebDAVConfig = Field(default_factory=WebDAVConfig)
    thumbnails: ThumbnailConfig = Field(default_factory=ThumbnailConfig)
    reverse_geocoding: ReverseGeocodingConfig = Field(
        default_factory=ReverseGeocodingConfig
    )


def load_config(config_path: Path) -> Config:
    if not config_path.exists():
        return Config()

    with open(config_path, "r") as config_file:
        data = yaml.safe_load(config_file) or {}

    return Config(**data)


def save_default_config(config_path: Path) -> None:
    config_path.parent.mkdir(parents=True, exist_ok=True)

    default_config = Config()
    config_dict = default_config.model_dump()

    with open(config_path, "w") as config_file:
        yaml.dump(config_dict, config_file, default_flow_style=False, sort_keys=False)
