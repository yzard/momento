from typing import Literal, Optional

from pydantic import BaseModel, ConfigDict


def to_camel(string: str) -> str:
    """Convert snake_case to camelCase."""
    components = string.split("_")
    return components[0] + "".join(x.title() for x in components[1:])


class CamelCaseModel(BaseModel):
    """Base model that serializes to camelCase."""

    model_config = ConfigDict(alias_generator=to_camel, populate_by_name=True, serialize_by_alias=True)


class MediaResponse(CamelCaseModel):
    id: int
    filename: str
    original_filename: str
    media_type: Literal["image", "video"]
    mime_type: Optional[str]
    width: Optional[int]
    height: Optional[int]
    file_size: Optional[int]
    duration_seconds: Optional[float]
    date_taken: Optional[str]
    gps_latitude: Optional[float]
    gps_longitude: Optional[float]
    camera_make: Optional[str]
    camera_model: Optional[str]
    iso: Optional[int]
    exposure_time: Optional[str]
    f_number: Optional[float]
    focal_length: Optional[float]
    gps_altitude: Optional[float]
    location_state: Optional[str]
    location_country: Optional[str]
    keywords: Optional[str]
    created_at: str


class MediaListRequest(BaseModel):
    cursor: Optional[str] = None
    limit: int = 50


class MediaListResponse(CamelCaseModel):
    items: list[MediaResponse]
    next_cursor: Optional[str]
    has_more: bool


class MediaGetRequest(BaseModel):
    media_id: int


class MediaUpdateRequest(BaseModel):
    media_id: int
    date_taken: Optional[str] = None
    gps_latitude: Optional[float] = None
    gps_longitude: Optional[float] = None


class MediaDeleteRequest(BaseModel):
    media_id: int


class TimelineGroup(CamelCaseModel):
    date: str
    media: list[MediaResponse]


class TimelineListRequest(BaseModel):
    cursor: Optional[str] = None
    limit: int = 100
    group_by: Literal["year", "month", "week", "day"] = "day"


class TimelineListResponse(CamelCaseModel):
    groups: list[TimelineGroup]
    next_cursor: Optional[str]
    has_more: bool


class GeoMediaResponse(CamelCaseModel):
    id: int
    thumbnail_path: Optional[str]
    thumbnail_data: Optional[str] = None
    latitude: float
    longitude: float
    date_taken: Optional[str]
    media_type: Literal["image", "video"]
    mime_type: Optional[str]
    original_filename: Optional[str]


class MapMediaResponse(CamelCaseModel):
    items: list[GeoMediaResponse]
