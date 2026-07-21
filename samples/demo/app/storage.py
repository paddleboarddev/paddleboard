"""In-memory storage and the aggregation logic behind the stats endpoint.

Deliberately dependency-free so the sample runs anywhere, including inside the
sandbox with no network access.
"""

from datetime import date, timedelta

from .models import PaddlerStats, Session, SessionCreate


class SessionStore:
    """Holds paddle sessions and answers questions about them."""

    def __init__(self) -> None:
        self._sessions: dict[int, Session] = {}
        self._next_id = 1

    def add(self, payload: SessionCreate) -> Session:
        session = Session(id=self._next_id, **payload.model_dump())
        self._sessions[session.id] = session
        self._next_id += 1
        return session

    def get(self, session_id: int) -> Session | None:
        return self._sessions.get(session_id)

    def list(self, paddler: str | None = None) -> list[Session]:
        sessions = sorted(self._sessions.values(), key=lambda s: (s.logged_on, s.id))
        if paddler is None:
            return sessions
        wanted = paddler.casefold()
        return [s for s in sessions if s.paddler.casefold() == wanted]

    def delete(self, session_id: int) -> bool:
        return self._sessions.pop(session_id, None) is not None

    def stats_for(self, paddler: str) -> PaddlerStats | None:
        """Summarize one paddler's history, including their best streak.

        A streak is consecutive calendar days with at least one session; two
        sessions on the same day count once.
        """
        sessions = self.list(paddler)
        if not sessions:
            return None

        distances = [s.distance_km for s in sessions]
        return PaddlerStats(
            paddler=sessions[0].paddler,
            sessions=len(sessions),
            total_km=round(sum(distances), 2),
            longest_km=max(distances),
            best_streak_days=longest_consecutive_run({s.logged_on for s in sessions}),
        )


def longest_consecutive_run(days: set[date]) -> int:
    """Return the length of the longest run of consecutive dates.

    Walks the sorted dates once and restarts the count whenever the gap to the
    previous date is anything other than a single day.
    """
    if not days:
        return 0

    ordered = sorted(days)
    best = current = 1
    for previous, day in zip(ordered, ordered[1:]):
        current = current + 1 if day - previous == timedelta(days=1) else 1
        best = max(best, current)
    return best
