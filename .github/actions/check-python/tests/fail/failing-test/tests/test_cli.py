"""The command line greets the name it is given."""

from sample.cli import greet, main


def test_greet() -> None:
    """Greet a name."""
    assert greet("Ada") == "Hi, Ada!"


def test_main_greets_the_world() -> None:
    """Greet the world without a name."""
    assert main([]) == 0
