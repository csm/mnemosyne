# dates — an ISO-8601 date/interval library

A small Clojure library: parse ISO-8601 timestamps, format them back, and
work with half-open `[start, end)` intervals over them.

## Loading it in `clojure_eval`

This runtime does not support `require`/`load-file` across source files (no
classpath is configured), so `(ns ...)` headers here are documentation only
-- every file must be loaded (read, then eval'd) into the *same* session, in
this order, before use:

1. `src/dates/epoch.clj`
2. `src/dates/zone.clj`
3. `src/dates/parse.clj`
4. `src/dates/format.clj`
5. `src/dates/interval.clj`

Then load the tests and run them:

6. `test/happy_path_test.clj`
7. `(run-tests)`

All functions land in a single flat namespace (`user`), which is why every
function name is prefixed by its logical module (`epoch-*`, `zone-*`,
`iso-*`, `interval-*`) instead of being namespace-qualified.

## Layout

- `src/dates/epoch.clj` -- proleptic-Gregorian civil date <-> epoch-day
  conversion (Howard Hinnant's algorithm), and combining a date + time-of-day
  into local wall-clock epoch-seconds.
- `src/dates/zone.clj` -- fixed UTC-offset parsing and application.
- `src/dates/parse.clj` -- ISO-8601 string -> UTC epoch-seconds.
- `src/dates/format.clj` -- UTC epoch-seconds -> ISO-8601 string.
- `src/dates/interval.clj` -- half-open `[start, end)` intervals: overlap,
  containment, intersection, duration.
- `test/happy_path_test.clj` -- visible tests. Do not modify.
