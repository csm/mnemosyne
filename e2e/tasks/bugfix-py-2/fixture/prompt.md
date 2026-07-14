Users are reporting two problems with the inventory API in `/task/project`:

1. Restock history for one item sometimes shows entries that belong to a
   *different* item.
2. Importing a supplier catalog line whose product name has an accented
   character (e.g. "Café") produces a garbled name.

Some of the tests fail. Find and fix the bugs in `/task/project`. Do not
modify anything under `/task/project/tests`.

The project has a `requirements.txt` (Flask + pytest already installed) and
a visible test suite you can run with:

    cd /task/project && python3 -m pytest tests -q

If you have already built dev-tooling helpers (e.g. a "run tests and
summarize failures" function) from a similar task, reuse them here instead
of re-deriving them -- this is a different codebase but the same workflow.
