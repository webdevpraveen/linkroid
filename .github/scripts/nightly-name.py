#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""A nightly's name from its number: a colour and a moniker for nightly releases
("Amber Vector"). Releases are named for release milestone (0.1 Genesis); nightlies
for active builds. 23 colours and 25 monikers, so 575 nightlies in a row
never share a name."""

import sys

COLOURS = [
    "Amber", "Golden", "Cyan", "Saffron", "Copper", "Coral", "Rose", "Ruby",
    "Plum", "Violet", "Indigo", "Azure", "Teal", "Jade", "Lilac", "Olive",
    "Ivory", "Pearl", "Silver", "Dusk", "Dawn", "Velvet", "Ember",
]
MONIKERS = [
    "Nexus", "Matrix", "Vector", "Pulse", "Orbit", "Signal", "Cipher", "Flux",
    "Helix", "Node", "Beacon", "Wave", "Relay", "Mesh", "Echo", "Prism",
    "Quantum", "Apex", "Spark", "Zenith", "Circuit", "Bridge", "Core", "Gate", "Horizon",
]


def name(number: int) -> str:
    return f"{COLOURS[number % len(COLOURS)]} {MONIKERS[number % len(MONIKERS)]}"


if __name__ == "__main__":
    print(name(int(sys.argv[1])))
