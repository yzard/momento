from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException, status
from fastapi.security import HTTPBasic, HTTPBasicCredentials

from momento_api.auth.dependencies import CurrentUser, get_config, get_current_user
from momento_api.auth.jwt import create_access_token, create_refresh_token, hash_refresh_token
from momento_api.auth.password import hash_password, verify_password
from momento_api.config import Config
from momento_api.database import execute_query, fetch_one, insert_returning_id
from momento_api.models.auth import ChangePasswordRequest, LogoutRequest, RefreshTokenRequest, TokenResponse

router = APIRouter(prefix="/user", tags=["user"])
_basic_auth = HTTPBasic()


@router.post("/authenticate", response_model=TokenResponse)
async def login(
    credentials: Annotated[HTTPBasicCredentials, Depends(_basic_auth)], config: Annotated[Config, Depends(get_config)]
) -> TokenResponse:
    user_row = fetch_one(
        "SELECT id, username, email, role, hashed_password, is_active FROM users WHERE username = ?",
        (credentials.username,),
    )

    if user_row is None or not verify_password(credentials.password, user_row["hashed_password"]):
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid credentials")

    if not user_row["is_active"]:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="User is inactive")

    access_token = create_access_token(user_row["id"], user_row["username"], user_row["role"], config)
    raw_refresh, token_hash, expires_at = create_refresh_token(user_row["id"], config)

    insert_returning_id(
        "INSERT INTO refresh_tokens (token_hash, user_id, expires_at) VALUES (?, ?, ?)",
        (token_hash, user_row["id"], expires_at.isoformat()),
    )

    return TokenResponse(access_token=access_token, refresh_token=raw_refresh)


@router.post("/refresh", response_model=TokenResponse)
async def refresh(request: RefreshTokenRequest, config: Annotated[Config, Depends(get_config)]) -> TokenResponse:
    token_hash = hash_refresh_token(request.refresh_token)

    token_row = fetch_one(
        """
        SELECT rt.id, rt.user_id, rt.expires_at, rt.revoked, u.username, u.role, u.is_active
        FROM refresh_tokens rt
        JOIN users u ON rt.user_id = u.id
        WHERE rt.token_hash = ?
        """,
        (token_hash,),
    )

    if token_row is None:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid refresh token")

    if token_row["revoked"]:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="Token has been revoked")

    if not token_row["is_active"]:
        raise HTTPException(status_code=status.HTTP_401_UNAUTHORIZED, detail="User is inactive")

    execute_query("UPDATE refresh_tokens SET revoked = 1 WHERE id = ?", (token_row["id"],))
    execute_query("DELETE FROM refresh_tokens WHERE revoked = 1 AND id = ?", (token_row["id"],))

    access_token = create_access_token(token_row["user_id"], token_row["username"], token_row["role"], config)
    raw_refresh, new_token_hash, expires_at = create_refresh_token(token_row["user_id"], config)

    insert_returning_id(
        "INSERT INTO refresh_tokens (token_hash, user_id, expires_at) VALUES (?, ?, ?)",
        (new_token_hash, token_row["user_id"], expires_at.isoformat()),
    )

    return TokenResponse(access_token=access_token, refresh_token=raw_refresh)


@router.post("/logout")
async def logout(request: LogoutRequest) -> dict:
    token_hash = hash_refresh_token(request.refresh_token)
    execute_query("UPDATE refresh_tokens SET revoked = 1 WHERE token_hash = ?", (token_hash,))
    return {"message": "Logged out successfully"}


@router.post("/change-password")
async def change_password(
    request: ChangePasswordRequest, current_user: Annotated[CurrentUser, Depends(get_current_user)]
) -> dict:
    user_row = fetch_one("SELECT hashed_password FROM users WHERE id = ?", (current_user.id,))

    if user_row is None or not verify_password(request.current_password, user_row["hashed_password"]):
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Current password is incorrect")

    if len(request.new_password) < 8:
        raise HTTPException(status_code=status.HTTP_400_BAD_REQUEST, detail="Password must be at least 8 characters")

    new_hash = hash_password(request.new_password)
    execute_query(
        "UPDATE users SET hashed_password = ?, must_change_password = 0 WHERE id = ?", (new_hash, current_user.id)
    )

    execute_query("UPDATE refresh_tokens SET revoked = 1 WHERE user_id = ?", (current_user.id,))

    return {"message": "Password changed successfully"}
