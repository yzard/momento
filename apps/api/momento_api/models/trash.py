from typing import Optional

from pydantic import BaseModel, ConfigDict, field_serializer
from pydantic.alias_generators import to_camel


class TrashMediaResponse(BaseModel):
    model_config = ConfigDict(
        alias_generator=to_camel,
        populate_by_name=True,
    )

    id: int
    filename: str
    original_filename: str
    media_type: str
    mime_type: Optional[str]
    width: Optional[int]
    height: Optional[int]
    file_size: Optional[int]
    duration_seconds: Optional[float]
    date_taken: Optional[str]
    deleted_at: str
    created_at: str


class TrashListResponse(BaseModel):
    model_config = ConfigDict(
        alias_generator=to_camel,
        populate_by_name=True,
    )

    items: list[TrashMediaResponse]
    total_count: int


class TrashRestoreRequest(BaseModel):
    media_ids: list[int]


class TrashDeleteRequest(BaseModel):
    media_ids: list[int]


class TrashResponse(BaseModel):
    message: str
    affected_count: int
