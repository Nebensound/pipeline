"""The command line of the sample."""

import argparse
import sys


def greet(name: str) -> int:
    """Return the greeting for a name."""
    return f"Hello, {name}!"


def main(argv: list[str] | None = None) -> int:
    """Greet the name on the command line, or the world."""
    parser = argparse.ArgumentParser(prog="sample", description="Greets someone.")
    parser.add_argument("name", nargs="?", default="world")
    arguments = parser.parse_args(argv)
    sys.stdout.write(greet(str(arguments.name)) + "\n")
    return 0
