from typing import Annotated

from fastapi import Depends, HTTPException, Query, Request, status
from fastapi.security import HTTPAuthorizationCredentials, HTTPBearer

from momento_api.auth.jwt import decode_access_token
from momento_api.config import Config
from momento_api.database import fetch_one

_bearer_scheme = HTTPBearer(auto_error=False)


def get_config(request: Request) -> Config:
    return request.app.state.config


class CurrentUser:
    def __init__(
        self, id: int, username: str, email: str, role: str, must_change_password: bool
    ):
        self.id = id
        self.username = username
        self.email = email
        self.role = role
        self.must_change_password = must_change_password


async def get_current_user(
    credentials: Annotated[
        HTTPAuthorizationCredentials | None, Depends(_bearer_scheme)
    ],
    config: Annotated[Config, Depends(get_config)],
    token: Annotated[str | None, Query()] = None,
) -> CurrentUser:
    token_str = None
    if credentials is not None:
        token_str = credentials.credentials
    elif token is not None:
        token_str = token

    if token_str is None:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED, detail="Not authenticated"
        )

    payload = decode_access_token(token_str, config)
    if payload is None:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid or expired token"
        )

    user_id = int(payload["sub"])
    user_row = fetch_one(
        "SELECT id, username, email, role, must_change_password, is_active FROM users WHERE id = ?",
        (user_id,),
    )

    if user_row is None:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED, detail="User not found"
        )

    if not user_row["is_active"]:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED, detail="User is inactive"
        )

    return CurrentUser(
        id=user_row["id"],
        username=user_row["username"],
        email=user_row["email"],
        role=user_row["role"],
        must_change_password=bool(user_row["must_change_password"]),
    )


async def require_admin(
    current_user: Annotated[CurrentUser, Depends(get_current_user)],
) -> CurrentUser:
    if current_user.role != "admin":
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN, detail="Admin access required"
        )
    return current_user
