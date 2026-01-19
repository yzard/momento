import threading
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException, Request, status

from momento_api.auth.dependencies import CurrentUser, get_config, require_admin
from momento_api.config import Config
from momento_api.models.imports import (
    ImportStatusResponse,
    ImportTriggerResponse,
    RegenerateRequest,
    RegenerateResponse,
    RegenerationStatusResponse,
)
from momento_api.processor.importer import get_import_status, is_import_running, run_local_import, run_webdav_import
from momento_api.processor.regenerator import (
    cancel_regeneration,
    clear_all_metadata_and_thumbnails,
    get_regeneration_status,
    is_regeneration_running,
    run_regeneration,
)

router = APIRouter(prefix="/import", tags=["import"])


@router.post("/local", response_model=ImportTriggerResponse)
async def trigger_local_import(
    admin: Annotated[CurrentUser, Depends(require_admin)], config: Annotated[Config, Depends(get_config)]
) -> ImportTriggerResponse:
    if is_import_running():
        raise HTTPException(status_code=status.HTTP_409_CONFLICT, detail="Import already in progress")

    thread = threading.Thread(
        target=run_local_import,
        kwargs={
            "user_id": admin.id,
            "thumbnail_max_size": config.thumbnails.max_size,
            "thumbnail_quality": config.thumbnails.quality,
            "video_frame_quality": config.thumbnails.video_frame_quality,
            "delete_after_import": True,
            "reverse_geo_config": config.reverse_geocoding,
        },
        daemon=True,
    )
    thread.start()

    return ImportTriggerResponse(message="Import started", status="running")


@router.post("/webdav", response_model=ImportTriggerResponse)
async def trigger_webdav_import(
    admin: Annotated[CurrentUser, Depends(require_admin)], config: Annotated[Config, Depends(get_config)]
) -> ImportTriggerResponse:
    if is_import_running():
        raise HTTPException(status_code=status.HTTP_409_CONFLICT, detail="Import already in progress")

    if not config.webdav.enabled:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="WebDAV import is not enabled")

    if not config.webdav.hostname:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="WebDAV hostname is not configured")

    thread = threading.Thread(
        target=run_webdav_import,
        kwargs={
            "user_id": admin.id,
            "hostname": config.webdav.hostname,
            "username": config.webdav.username,
            "password": config.webdav.password,
            "remote_path": config.webdav.remote_path,
            "thumbnail_max_size": config.thumbnails.max_size,
            "thumbnail_quality": config.thumbnails.quality,
            "video_frame_quality": config.thumbnails.video_frame_quality,
            "reverse_geo_config": config.reverse_geocoding,
        },
        daemon=True,
    )
    thread.start()

    return ImportTriggerResponse(message="WebDAV import started", status="running")


@router.post("/status", response_model=ImportStatusResponse)
async def get_import_job_status(_: Annotated[CurrentUser, Depends(require_admin)]) -> ImportStatusResponse:
    job = get_import_status()
    return ImportStatusResponse(
        status=job.status.value,
        total_files=job.total_files,
        processed_files=job.processed_files,
        successful_imports=job.successful_imports,
        failed_imports=job.failed_imports,
        started_at=job.started_at.isoformat() if job.started_at else None,
        completed_at=job.completed_at.isoformat() if job.completed_at else None,
        errors=job.errors,
    )


@router.post("/regenerate", response_model=RegenerateResponse)
async def trigger_regeneration(
    request: RegenerateRequest,
    admin: Annotated[CurrentUser, Depends(require_admin)],
    config: Annotated[Config, Depends(get_config)],
) -> RegenerateResponse:
    if is_regeneration_running():
        raise HTTPException(status_code=status.HTTP_409_CONFLICT, detail="Regeneration already in progress")

    thread = threading.Thread(
        target=run_regeneration, kwargs={"missing_only": request.missing_only, "config": config}, daemon=True
    )
    thread.start()

    return RegenerateResponse(message="Regeneration started", status="running")


@router.post("/regenerate/status", response_model=RegenerationStatusResponse)
async def get_regeneration_job_status(_: Annotated[CurrentUser, Depends(require_admin)]) -> RegenerationStatusResponse:
    job = get_regeneration_status()
    return RegenerationStatusResponse(
        status=job.status.value,
        total_media=job.total_media,
        processed_media=job.processed_media,
        updated_metadata=job.updated_metadata,
        generated_thumbnails=job.generated_thumbnails,
        updated_tags=job.updated_tags,
        started_at=job.started_at.isoformat() if job.started_at else None,
        completed_at=job.completed_at.isoformat() if job.completed_at else None,
        errors=job.errors,
    )


@router.post("/regenerate/cancel", response_model=RegenerateResponse)
async def cancel_regeneration_job(_: Annotated[CurrentUser, Depends(require_admin)]) -> RegenerateResponse:
    if cancel_regeneration():
        return RegenerateResponse(message="Cancellation requested", status="cancelling")
    return RegenerateResponse(message="No regeneration job to cancel", status="idle")


@router.post("/reset", response_model=RegenerateResponse)
async def trigger_reset(
    admin: Annotated[CurrentUser, Depends(require_admin)], config: Annotated[Config, Depends(get_config)]
) -> RegenerateResponse:
    """Clear all metadata and thumbnails, then regenerate everything."""
    if is_regeneration_running():
        raise HTTPException(status_code=status.HTTP_409_CONFLICT, detail="Regeneration already in progress")

    if is_import_running():
        raise HTTPException(status_code=status.HTTP_409_CONFLICT, detail="Import already in progress")

    def clean_and_regenerate():
        clear_all_metadata_and_thumbnails()
        run_regeneration(missing_only=False, config=config)

    thread = threading.Thread(target=clean_and_regenerate, daemon=True)
    thread.start()

    return RegenerateResponse(message="Cleaning and regeneration started", status="running")
