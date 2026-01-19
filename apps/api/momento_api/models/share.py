from typing import Optional

from pydantic import BaseModel


class ShareLinkResponse(BaseModel):
    id: int
    token: str
    media_id: Optional[int]
    album_id: Optional[int]
    has_password: bool
    expires_at: Optional[str]
    view_count: int
    created_at: str


class ShareCreateRequest(BaseModel):
    media_id: Optional[int] = None
    album_id: Optional[int] = None
    password: Optional[str] = None
    expires_in_days: Optional[int] = None


class ShareDeleteRequest(BaseModel):
    share_id: int


class ShareListResponse(BaseModel):
    shares: list[ShareLinkResponse]


class ShareVerifyRequest(BaseModel):
    password: str
