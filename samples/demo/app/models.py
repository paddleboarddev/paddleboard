"""Data models for the Paddle Log API."""

from datetime import date
from enum import Enum

from pydantic import BaseModel, Field


class Conditions(str, Enum):
    """How the water looked when the session started."""

    GLASSY = "glassy"
    CHOPPY = "choppy"
    WINDY = "windy"


class SessionCreate(BaseModel):
    """Payload for logging a new paddle session."""

    paddler: str = Field(min_length=1, max_length=60)
    spot: str = Field(min_length=1, max_length=80)
    logged_on: date
    distance_km: float = Field(gt=0, le=200)
    conditions: Conditions = Conditions.GLASSY


class Session(SessionCreate):
    """A stored session, with its assigned identifier."""

    id: int


class PaddlerStats(BaseModel):
    """Aggregate figures for a single paddler."""

    paddler: str
    sessions: int
    total_km: float
    longest_km: float
    best_streak_days: int
