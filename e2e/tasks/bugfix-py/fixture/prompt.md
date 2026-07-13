Users are reporting two problems with the task API in `/task/project`:

1. Paginated task listings are missing items — for a page that should be
   full, something is dropped.
2. The "due today" check is wrong for some users near midnight in their
   local timezone.

Some of the tests fail. Find and fix the bugs in `/task/project`. Do not
modify anything under `/task/project/tests`.

The project has a `requirements.txt` (Flask + pytest already installed) and
a visible test suite you can run with:

    cd /task/project && python3 -m pytest tests -q
