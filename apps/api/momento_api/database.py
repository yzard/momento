import sqlite3
from pathlib import Path
from threading import local
from typing import Optional

from momento_api.constants import DATABASE_PATH

_thread_local = local()


def get_connection() -> sqlite3.Connection:
    if not hasattr(_thread_local, "connection") or _thread_local.connection is None:
        _thread_local.connection = sqlite3.connect(DATABASE_PATH, check_same_thread=False)
        _thread_local.connection.row_factory = sqlite3.Row
        _thread_local.connection.execute("PRAGMA foreign_keys = ON")
    return _thread_local.connection


def close_database() -> None:
    if hasattr(_thread_local, "connection") and _thread_local.connection is not None:
        _thread_local.connection.close()
        _thread_local.connection = None


def init_database(schema_path: Path) -> None:
    if not schema_path.exists():
        return

    conn = get_connection()
    with open(schema_path, "r") as f:
        conn.executescript(f.read())
    conn.commit()


def ensure_media_columns() -> None:
    conn = get_connection()
    existing = {row["name"] for row in conn.execute("PRAGMA table_info(media)")}
    columns: dict[str, str] = {
        "iso": "INTEGER",
        "exposure_time": "TEXT",
        "f_number": "REAL",
        "focal_length": "REAL",
        "gps_altitude": "REAL",
        "location_state": "TEXT",
        "location_country": "TEXT",
        "keywords": "TEXT",
        "deleted_at": "TEXT",
    }
    for column_name, column_type in columns.items():
        if column_name in existing:
            continue
        conn.execute(f"ALTER TABLE media ADD COLUMN {column_name} {column_type}")
    conn.commit()


def execute_query(sql: str, params: tuple) -> sqlite3.Cursor:
    conn = get_connection()
    return conn.execute(sql, params)


def execute_many(sql: str, params_list: list[tuple]) -> None:
    conn = get_connection()
    conn.executemany(sql, params_list)
    conn.commit()


def fetch_one(sql: str, params: tuple) -> Optional[sqlite3.Row]:
    cursor = execute_query(sql, params)
    return cursor.fetchone()


def fetch_all(sql: str, params: tuple) -> list[sqlite3.Row]:
    cursor = execute_query(sql, params)
    return cursor.fetchall()


def insert_returning_id(sql: str, params: tuple) -> int:
    conn = get_connection()
    cursor = conn.execute(sql, params)
    conn.commit()
    if cursor.lastrowid is None:
        raise RuntimeError("Insert failed: no lastrowid returned")
    return cursor.lastrowid
