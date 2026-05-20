"""Exception hierarchy for the public :mod:`hammerhead` SDK.

Every error the SDK raises on purpose is a :class:`HammerheadError`, so a
caller can catch the whole family with a single ``except`` clause::

    from hammerhead import Bot, HammerheadError

    bot = Bot()
    try:
        bot.play((0, 0))
        bot.play((0, 0))          # occupied
    except HammerheadError as exc:
        print(f"rejected: {exc}")

Lower-level engine failures surface as :class:`IllegalMoveError`. Calling
a query that requires a live game after the game has ended raises
:class:`GameOverError`. :class:`NotationError` is reserved for the
string-notation parsers (BKE / BSN / HXN), which are not implemented yet.
"""

from __future__ import annotations


class HammerheadError(Exception):
    """Base class for every error raised deliberately by the SDK.

    Catch this to handle any SDK-level failure without naming each
    subclass individually.
    """


class IllegalMoveError(HammerheadError):
    """Raised when a stone cannot be placed at the requested coordinate.

    Fires when the target cell is already occupied or lies outside the
    engine's playable range.
    """


class GameOverError(HammerheadError):
    """Raised when a move or search is attempted on a finished game.

    Once a side has won, the position is terminal: :meth:`Bot.play` and
    :meth:`Bot.suggest` reject further activity until :meth:`Bot.reset`
    or :meth:`Bot.undo` reopens the game.
    """


class NotationError(HammerheadError):
    """Raised when a move string cannot be interpreted.

    String notation (BKE / BSN / HXN) is not implemented yet, so today
    this fires whenever a :class:`str` is passed where a ``(q, r)``
    coordinate tuple is expected. Once the ``hammerhead.notation``
    parsers ship it will also cover malformed notation strings.
    """
