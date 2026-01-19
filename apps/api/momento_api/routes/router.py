from fastapi import APIRouter

from momento_api.routes.albums import router as albums_router
from momento_api.routes.auth import router as auth_router
from momento_api.routes.imports import router as imports_router
from momento_api.routes.map import router as map_router
from momento_api.routes.media import router as media_router
from momento_api.routes.media import thumbnail_router, preview_router
from momento_api.routes.public import router as public_router
from momento_api.routes.share import router as share_router
from momento_api.routes.tags import router as tags_router
from momento_api.routes.timeline import router as timeline_router
from momento_api.routes.trash import router as trash_router
from momento_api.routes.users import router as users_router

api_router = APIRouter()
api_router.include_router(auth_router)
api_router.include_router(users_router)
api_router.include_router(media_router)
api_router.include_router(thumbnail_router)
api_router.include_router(preview_router)
api_router.include_router(timeline_router)
api_router.include_router(albums_router)
api_router.include_router(tags_router)
api_router.include_router(map_router)
api_router.include_router(share_router)
api_router.include_router(public_router)
api_router.include_router(imports_router)
api_router.include_router(trash_router)
