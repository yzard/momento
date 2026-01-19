import hashlib
import secrets
from datetime import datetime, timedelta, timezone
from typing import Optional

from jose import JWTError, jwt

from momento_api.config import Config


def create_access_token(user_id: int, username: str, role: str, config: Config) -> str:
    expire = datetime.now(timezone.utc) + timedelta(minutes=config.security.access_token_expire_minutes)
    payload = {"sub": str(user_id), "username": username, "role": role, "exp": expire, "type": "access"}
    return jwt.encode(payload, config.security.secret_key, algorithm=config.security.algorithm)


def create_refresh_token(user_id: int, config: Config) -> tuple[str, str, datetime]:
    raw_token = secrets.token_urlsafe(32)
    token_hash = hashlib.sha256(raw_token.encode()).hexdigest()
    expire = datetime.now(timezone.utc) + timedelta(days=config.security.refresh_token_expire_days)
    return raw_token, token_hash, expire


def decode_access_token(token: str, config: Config) -> Optional[dict]:
    payload = _decode_token(token, config)
    if payload is None:
        return None
    if payload.get("type") != "access":
        return None
    return payload


def _decode_token(token: str, config: Config) -> Optional[dict]:
    try:
        return jwt.decode(token, config.security.secret_key, algorithms=[config.security.algorithm])
    except JWTError:
        return None


def hash_refresh_token(token: str) -> str:
    return hashlib.sha256(token.encode()).hexdigest()
