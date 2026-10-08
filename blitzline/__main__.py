"""Start the CLI when Python runs the Blitzline package as a module.

Scope statement: forward module execution to Click.
Included: the guarded console entry.
Excluded: command definitions (cli.py) and pipeline logic (core.py).
Start here: run `python -m blitzline --help` to see available commands.
"""

from blitzline.cli import cli

if __name__ == "__main__":
    cli()
