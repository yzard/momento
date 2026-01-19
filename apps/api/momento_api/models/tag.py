from pydantic import BaseModel


class TagResponse(BaseModel):
    id: int
    name: str
    created_at: str


class TagCreateRequest(BaseModel):
    name: str


class TagDeleteRequest(BaseModel):
    tag_id: int


class TagAddToMediaRequest(BaseModel):
    tag_id: int
    media_ids: list[int]


class TagRemoveFromMediaRequest(BaseModel):
    tag_id: int
    media_ids: list[int]


class TagListResponse(BaseModel):
    tags: list[TagResponse]
