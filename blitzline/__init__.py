"""Identify the Blitzline package without starting the application.

Scope statement: expose package metadata only.
Included: the installed application version.
Excluded: CLI startup (cli.py), pipeline execution (core.py), and I/O.
Start here: core.run_pipeline provides the application API.
"""

__version__ = "0.1.0"
