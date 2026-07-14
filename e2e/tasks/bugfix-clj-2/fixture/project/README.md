# schedule — a weekly recurring time-slot library

A small Clojure library, built on the same epoch-day arithmetic as the
`bugfix-clj` task's `dates` library: recurring weekly time-of-day slots
(day-of-week + a half-open minute-of-day window), day-of-week lookup, and
slot-conflict checking.

## Loading it in `clojure_eval`

This runtime does not support `require`/`load-file` across source files (no
classpath is configured), so `(ns ...)` headers here are documentation only
-- every file must be loaded (read, then eval'd) into the *same* session, in
this order, before use:

1. `src/dates/epoch.clj`
2. `src/schedule/weekly.clj`

Then load the tests and run them:

3. `test/happy_path_test.clj`
4. `(run-tests)`

All functions land in a single flat namespace (`user`), which is why every
function name is prefixed by its logical module (`epoch-*`, `weekly-*`)
instead of being namespace-qualified.

## Layout

- `src/dates/epoch.clj` -- proleptic-Gregorian civil date <-> epoch-day
  conversion (Howard Hinnant's algorithm). Identical to the same file in
  the `bugfix-clj` task; not itself buggy.
- `src/schedule/weekly.clj` -- weekly recurring slots: construction,
  day-of-week lookup, occurrence checks, and conflict detection.
- `test/happy_path_test.clj` -- visible tests. Do not modify.
