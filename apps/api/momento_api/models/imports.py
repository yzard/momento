from typing import Optional

from pydantic import BaseModel


class ImportStatusResponse(BaseModel):
    status: str
    total_files: int
    processed_files: int
    successful_imports: int
    failed_imports: int
    started_at: Optional[str]
    completed_at: Optional[str]
    errors: list[str]


class ImportTriggerResponse(BaseModel):
    message: str
    status: str


class RegenerateRequest(BaseModel):
    missing_only: bool = True


class RegenerateResponse(BaseModel):
    message: str
    status: str


class RegenerationStatusResponse(BaseModel):
    status: str
    total_media: int
    processed_media: int
    updated_metadata: int
    generated_thumbnails: int
    updated_tags: int
    started_at: Optional[str]
    completed_at: Optional[str]
    errors: list[str]
