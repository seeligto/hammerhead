"""Curated opening library for the HARNESS (Phase 28E-2 Stage 0).

NOT an opening book in the bot. This is HARNESS-side opening diversity:
``run_match`` / ``run_match_parallel`` use ``pick_opening(pair_seed)`` to
choose a forced opening per pair, applies it to both bots before the first
search call, and the two games of the pair share that same opening
(color-swapped). Eliminates the Phase 28E-1 DIAG-1 fixed-depth determinism
collapse — distinct trajectories per pair.

Each opening is a sequence of ``(player, q, r)`` plies starting from an
empty board. Curation source: ``refs/The HeXOpedia.pdf`` §6 (Openings).

Coordinate convention
---------------------
Hammerhead uses **axial** coordinates ``(q, r)``; the third cube coord
``s = -q - r`` is implicit (``coords.rs``). Six neighbours of ``(0, 0)``:

    A0  ( 1,  0)   East
    A1  ( 1, -1)   North-East
    A2  ( 0, -1)   North-West
    A3  (-1,  0)   West
    A4  (-1,  1)   South-West
    A5  ( 0,  1)   South-East

X is "Player 1" / Red. O is "Player 2" / Blue. X's first ply MUST be at
the origin (``coords::ORIGIN`` enforced in ``board.rs:43``); per HeXOpedia
§1.2 (Turn Parity) that single piece is the entire first turn, then O
plays two, then X plays two, and so on.

BKE notation lines in the docstrings are *labels*, not parser input —
the axial tuples below were hand-mapped from each opening's HeXOpedia
diagram. ``test_openings.py`` exercises every opening through the live
engine to guarantee legality.
"""

from __future__ import annotations

from dataclasses import dataclass

# ─────────────────────────────────────────────────────────────────────────────
# Types
# ─────────────────────────────────────────────────────────────────────────────

Player = str  # "X" or "O"
Coord = tuple[int, int]
Ply = tuple[Player, int, int]


@dataclass(frozen=True, slots=True)
class Opening:
    """One named opening = label + ordered ply list.

    The ``plies`` list starts from an empty board. Every opening begins
    with ``("X", 0, 0)`` per HeXOpedia §1.2.
    """

    name: str
    plies: tuple[Ply, ...]
    cite: str  # HeXOpedia section reference


# ─────────────────────────────────────────────────────────────────────────────
# A-ring axial helpers (BKE A0..A5 → axial)
# ─────────────────────────────────────────────────────────────────────────────
#
# Used only as a reference table for human eyes; not imported elsewhere.
#
#     A0 = ( 1,  0)
#     A1 = ( 1, -1)
#     A2 = ( 0, -1)
#     A3 = (-1,  0)
#     A4 = (-1,  1)
#     A5 = ( 0,  1)
#
# B-ring and beyond are documented inline per opening.


# ─────────────────────────────────────────────────────────────────────────────
# Curated openings — HeXOpedia §6 (verbatim names)
# ─────────────────────────────────────────────────────────────────────────────

_OPENINGS: tuple[Opening, ...] = (
    # § 6.3 Close-Quarters Play -------------------------------------------------
    Opening(
        name="Pair",
        # BKE: x Z0 o A0 A1  — O places adjacent A-ring neighbours touching.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),     # A0
            ("O", 1, -1),    # A1
        ),
        cite="HeXOpedia §6.3 Pair Opening (A0 A1)",
    ),
    Opening(
        name="ClosedGame",
        # BKE: x Z0 o A0 A2  — O leaves a 1-hex gap (the "Closed Game" stem).
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),     # A0
            ("O", 0, -1),    # A2
        ),
        cite="HeXOpedia §6.3 Closed Game (A0 A2)",
    ),
    Opening(
        name="ClosedMainLine",
        # BKE: x Z0 o A0 A2 x A1 A4  — stem of the entire Sword Family.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),     # A0
            ("O", 0, -1),    # A2
            ("X", 1, -1),    # A1
            ("X", -1, 1),    # A4
        ),
        cite="HeXOpedia §6.1 Closed Game Main Line (A0 A2 x A1 A4)",
    ),
    # § 6.1 The Sword Family ----------------------------------------------------
    # All four Swords share the Main Line stem plus an O B-ring pair.
    # B-ring labels (12 hexes, ring distance 2 from origin) — minimal subset
    # used by the curated swords:
    #     B2  = ( 2, -2)   the "handle" anchor opposite the X cluster
    #     B8  = (-2,  0)   the Dagger wing (tight)
    #     C12 = (-3,  1)   the Sword wing (one-gap)
    #     D16 = (-4,  2)   the Longsword wing (wide, "long blade")
    #     E20 = (-5,  3)   the Wrongsword/Curtana wing (one further still)
    Opening(
        name="Longsword",
        # BKE: ... o B2 D16  — the canonical, most popular Sword variant.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),     # A0
            ("O", 0, -1),    # A2
            ("X", 1, -1),    # A1
            ("X", -1, 1),    # A4
            ("O", 2, -2),    # B2
            ("O", -4, 2),    # D16
        ),
        cite="HeXOpedia §6.1 Longsword Opening (... o B2 D16)",
    ),
    Opening(
        name="Shortsword",
        # BKE: ... o B2 B8  — tightest Sword: O's wings are adjacent in ring B.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),
            ("O", 0, -1),
            ("X", 1, -1),
            ("X", -1, 1),
            ("O", 2, -2),    # B2
            ("O", -2, 0),    # B8
        ),
        cite="HeXOpedia §6.1 Shortsword / Dagger (... o B2 B8)",
    ),
    Opening(
        name="Sword",
        # BKE: ... o B2 C12  — middle-ground (one-gap between wings).
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),
            ("O", 0, -1),
            ("X", 1, -1),
            ("X", -1, 1),
            ("O", 2, -2),    # B2
            ("O", -3, 1),    # C12
        ),
        cite="HeXOpedia §6.1 Sword Opening (... o B2 C12)",
    ),
    Opening(
        name="Wrongsword",
        # BKE: ... o B2 E20  — six hexes apart, igorex95's "wrong" sword.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),
            ("O", 0, -1),
            ("X", 1, -1),
            ("X", -1, 1),
            ("O", 2, -2),    # B2
            ("O", -5, 3),    # E20
        ),
        cite="HeXOpedia §6.1 Wrongsword / Curtana (... o B2 E20)",
    ),
    # § 6.2 The Firearms Family -------------------------------------------------
    Opening(
        name="Pistol",
        # BKE: x Z0 o A0 B2  — slightly offset, asymmetric.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),     # A0
            ("O", 2, -2),    # B2
        ),
        cite="HeXOpedia §6.2 Pistol Opening (A0 B2)",
    ),
    Opening(
        name="Shotgun",
        # BKE: x Z0 o A0 C5  — wider asymmetric spread into the C-ring.
        # C5 sits in the C-ring (radius 3); chosen along the same diagonal as
        # A0 to honour the "wide triangles" intent.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),     # A0
            ("O", 3, -1),    # C-ring hex on the +q,+s diagonal
        ),
        cite="HeXOpedia §6.2 Shotgun (A0 C5)",
    ),
    Opening(
        name="Revolver",
        # BKE: x Z0 o A0 C2  — tighter than Shotgun, still C-ring.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),     # A0
            ("O", 2, -3),    # C-ring "tight" diagonal placement
        ),
        cite="HeXOpedia §6.2 Revolver (A0 C2)",
    ),
    Opening(
        name="PistolSnail",
        # BKE: ... x A1 A5  — X's curving "Snail" response to the Pistol.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),
            ("O", 2, -2),
            ("X", 1, -1),    # A1
            ("X", 0, 1),     # A5
        ),
        cite="HeXOpedia §6.2 Snail response (... x A1 A5)",
    ),
    # § 6.4 The Island Gambit ---------------------------------------------------
    Opening(
        name="IslandGambit",
        # BKE: x Z0 o E0 E1  — distant 2-cell colony in the E-ring.
        # E-ring is radius 5; "E0" anchored on +q axis, "E1" its CCW neighbour.
        plies=(
            ("X", 0, 0),
            ("O", 5, 0),     # E0 (radius 5 on +q axis)
            ("O", 5, -1),    # E1 (adjacent in the E-ring)
        ),
        cite="HeXOpedia §6.4 Island Gambit (E0 E1)",
    ),
    Opening(
        name="NearIsland",
        # BKE: x Z0 o C0 C1  — safer C-ring variant of the Island Gambit.
        plies=(
            ("X", 0, 0),
            ("O", 3, 0),     # C0
            ("O", 3, -1),    # C1
        ),
        cite="HeXOpedia §6.4 Near Island (C0 C1)",
    ),
    # § 6.5 Psychological Openings ----------------------------------------------
    Opening(
        name="Marge",
        # BKE: x Z0 o A0 B0  — visually confusing, slightly bad per HeXOpedia.
        # B0 on the +q axis (radius 2, the B-ring hex directly past A0).
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),     # A0
            ("O", 2, 0),     # B0
        ),
        cite="HeXOpedia §6.5 Marge (A0 B0)",
    ),
    Opening(
        name="Eclipse",
        # BKE: x Z0 o C0 K0  — massive opening swallowing the outer rings.
        # K is the 11th ring (radius 11).
        plies=(
            ("X", 0, 0),
            ("O", 3, 0),     # C0 (radius 3 on +q)
            ("O", 11, 0),    # K0 (radius 11 on +q)
        ),
        cite="HeXOpedia §6.5 Eclipse (C0 K0)",
    ),
    Opening(
        name="C_and_B",
        # BKE: x Z0 o A0 A3  — defensive, spaced-out "101" line.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),     # A0
            ("O", -1, 0),    # A3
        ),
        cite="HeXOpedia §6.5 C&B / 101 Opening (A0 A3)",
    ),
    # § 6.3 Sub-variations ------------------------------------------------------
    Opening(
        name="PairSideStep",
        # BKE: ... x A2 B3  — X steps to the side after O's Pair.
        # B3 chosen as the B-ring hex sitting on the +A2 diagonal.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),
            ("O", 1, -1),
            ("X", 0, -1),    # A2
            ("X", 1, -2),    # B3 (one further out from A1 along NE diag)
        ),
        cite="HeXOpedia §6.3 Pair Side-Step Variation (... x A2 B3)",
    ),
    Opening(
        name="PairCShift",
        # BKE: ... x C4 C5  — X shifts entirely to the C-ring after Pair.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),
            ("O", 1, -1),
            ("X", 3, -2),    # C-ring hex
            ("X", 2, -1),    # C-ring neighbour
        ),
        cite="HeXOpedia §6.3 Pair C-Shift Variation (... x C4 C5)",
    ),
    # Control openings ----------------------------------------------------------
    # Three symmetric "control" variants. Not in HeXOpedia by name but
    # mechanically near-identical to the existing A-ring family (Pair / Closed
    # Game / C&B) — they exist to widen the diversity floor for stages 1-3 and
    # are anchored to the same A-ring geometry, so legality is invariant under
    # the engine's 12-fold symmetry. NO new theoretical claim.
    Opening(
        name="Control_A0A4",
        # X at origin; O on opposite-180° A-ring neighbours of the Pair stem.
        plies=(
            ("X", 0, 0),
            ("O", 1, 0),     # A0
            ("O", -1, 1),    # A4 (180° from A1)
        ),
        cite="HeXOpedia §6 (control variant; A0-A4 mirror of Pair stem)",
    ),
    Opening(
        name="Control_A2A5",
        # X at origin; O on a 60°-rotated Closed-Game-equivalent pair.
        plies=(
            ("X", 0, 0),
            ("O", 0, -1),    # A2
            ("O", 0, 1),     # A5
        ),
        cite="HeXOpedia §6 (control variant; A2-A5 rotation of Closed Game)",
    ),
)


# Public, immutable view.
OPENINGS: tuple[Opening, ...] = _OPENINGS


# ─────────────────────────────────────────────────────────────────────────────
# Selection
# ─────────────────────────────────────────────────────────────────────────────


def pick_opening(seed: int) -> Opening:
    """Deterministic opening selection from a (typically per-pair) seed.

    Uses modulo over ``OPENINGS`` for clean cycling — preferred to
    ``random.Random.choice`` because per-pair-deterministic indexing makes
    it trivial to reproduce a specific pair from its seed alone, and
    eliminates RNG-state coupling between pair scheduling and any other
    randomness in the harness.
    """
    if not OPENINGS:
        raise RuntimeError("OPENINGS catalog is empty")
    return OPENINGS[seed % len(OPENINGS)]


def opening_count() -> int:
    """Number of curated openings (test fixture helper)."""
    return len(OPENINGS)
