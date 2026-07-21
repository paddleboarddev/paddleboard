"""Paddle Log — a tiny FastAPI service for logging paddleboard sessions.

This is the PaddleBoard demo sample. It is intentionally small and boring so a
presenter can focus on the editor's features rather than the app's behavior.
See ../README.md for the walkthrough.
"""

from fastapi import FastAPI, HTTPException

from .models import PaddlerStats, Session, SessionCreate
from .storage import SessionStore

app = FastAPI(title="Paddle Log", version="1.0.0")
store = SessionStore()


@app.get("/healthz")
def healthz() -> dict[str, str]:
    return {"status": "ok"}


@app.post("/sessions", response_model=Session, status_code=201)
def create_session(payload: SessionCreate) -> Session:
    return store.add(payload)


@app.get("/sessions", response_model=list[Session])
def list_sessions(paddler: str | None = None) -> list[Session]:
    return store.list(paddler)


@app.get("/sessions/{session_id}", response_model=Session)
def get_session(session_id: int) -> Session:
    session = store.get(session_id)
    if session is None:
        raise HTTPException(status_code=404, detail="session not found")
    return session


@app.delete("/sessions/{session_id}", status_code=204)
def delete_session(session_id: int) -> None:
    if not store.delete(session_id):
        raise HTTPException(status_code=404, detail="session not found")


@app.get("/paddlers/{paddler}/stats", response_model=PaddlerStats)
def paddler_stats(paddler: str) -> PaddlerStats:
    stats = store.stats_for(paddler)
    if stats is None:
        raise HTTPException(status_code=404, detail="no sessions for that paddler")
    return stats
