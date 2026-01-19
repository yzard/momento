from typing import Literal, Optional

from pydantic import BaseModel, EmailStr


class UserResponse(BaseModel):
    id: int
    username: str
    email: str
    role: Literal["admin", "user"]
    must_change_password: bool
    is_active: bool
    created_at: str


class UserCreateRequest(BaseModel):
    username: str
    email: EmailStr
    password: str
    role: Literal["admin", "user"] = "user"


class UserUpdateRequest(BaseModel):
    role: Optional[Literal["admin", "user"]] = None
    is_active: Optional[bool] = None


class UserDeleteRequest(BaseModel):
    user_id: int


class UserListResponse(BaseModel):
    users: list[UserResponse]
