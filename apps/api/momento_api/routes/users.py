from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException, status

from momento_api.auth.dependencies import CurrentUser, get_current_user, require_admin
from momento_api.auth.password import hash_password
from momento_api.database import execute_query, fetch_all, fetch_one, get_connection, insert_returning_id
from momento_api.models.user import (
    UserCreateRequest,
    UserDeleteRequest,
    UserListResponse,
    UserResponse,
    UserUpdateRequest,
)

router = APIRouter(prefix="/user", tags=["user"])


def _row_to_user_response(row) -> UserResponse:
    return UserResponse(
        id=row["id"],
        username=row["username"],
        email=row["email"],
        role=row["role"],
        must_change_password=bool(row["must_change_password"]),
        is_active=bool(row["is_active"]),
        created_at=row["created_at"],
    )


@router.post("/create", response_model=UserResponse)
async def create_user(request: UserCreateRequest, _: Annotated[CurrentUser, Depends(require_admin)]) -> UserResponse:
    existing = fetch_one("SELECT id FROM users WHERE username = ? OR email = ?", (request.username, request.email))
    if existing:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Username or email already exists")

    if len(request.password) < 8:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Password must be at least 8 characters")

    hashed = hash_password(request.password)
    user_id = insert_returning_id(
        "INSERT INTO users (username, email, hashed_password, role, must_change_password) VALUES (?, ?, ?, ?, 1)",
        (request.username, request.email, hashed, request.role),
    )

    user_row = fetch_one("SELECT * FROM users WHERE id = ?", (user_id,))
    if user_row is None:
        raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail="Failed to create user")

    return _row_to_user_response(user_row)


@router.post("/list", response_model=UserListResponse)
async def list_users(_: Annotated[CurrentUser, Depends(require_admin)]) -> UserListResponse:
    rows = fetch_all("SELECT * FROM users ORDER BY created_at DESC", ())
    return UserListResponse(users=[_row_to_user_response(row) for row in rows])


@router.post("/get", response_model=UserResponse)
async def get_current_user_profile(current_user: Annotated[CurrentUser, Depends(get_current_user)]) -> UserResponse:
    user_row = fetch_one("SELECT * FROM users WHERE id = ?", (current_user.id,))
    if user_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="User not found")
    return _row_to_user_response(user_row)


@router.post("/update", response_model=UserResponse)
async def update_user(
    user_id: int, request: UserUpdateRequest, admin: Annotated[CurrentUser, Depends(require_admin)]
) -> UserResponse:
    user_row = fetch_one("SELECT * FROM users WHERE id = ?", (user_id,))
    if user_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="User not found")

    if user_id == admin.id and request.role == "user":
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Cannot demote yourself")

    updates: list[str] = []
    params: list[object] = []

    if request.role is not None:
        updates.append("role = ?")
        params.append(request.role)

    if request.is_active is not None:
        if user_id == admin.id and not request.is_active:
            raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Cannot deactivate yourself")
        updates.append("is_active = ?")
        params.append(1 if request.is_active else 0)

    if not updates:
        return _row_to_user_response(user_row)

    params.append(user_id)
    execute_query(f"UPDATE users SET {', '.join(updates)} WHERE id = ?", tuple(params))
    get_connection().commit()

    updated_row = fetch_one("SELECT * FROM users WHERE id = ?", (user_id,))
    if updated_row is None:
        raise HTTPException(status_code=status.HTTP_500_INTERNAL_SERVER_ERROR, detail="Failed to update user")

    return _row_to_user_response(updated_row)


@router.post("/delete")
async def delete_user(request: UserDeleteRequest, admin: Annotated[CurrentUser, Depends(require_admin)]) -> dict:
    if request.user_id == admin.id:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Cannot delete yourself")

    user_row = fetch_one("SELECT id FROM users WHERE id = ?", (request.user_id,))
    if user_row is None:
        raise HTTPException(status_code=status.HTTP_404_NOT_FOUND, detail="User not found")

    execute_query("DELETE FROM users WHERE id = ?", (request.user_id,))
    get_connection().commit()

    return {"message": "User deleted successfully"}
