There is a small Clojure library at `/task/project` (an ISO-8601
date-parsing and interval library, see `/task/project/README.md` for the
file layout and load order). Users report two problems:

1. Interval overlap checks are sometimes wrong at the boundary between two
   adjacent intervals.
2. Parsing timestamps with an explicit UTC offset (not `Z`) sometimes gives
   the wrong instant.

Find and fix the bugs. Do not modify anything under `/task/project/test`.

This is exactly the kind of code `clojure_eval`'s persistent runtime is
good at: load the source files in the order `README.md` describes, then
load and run `/task/project/test/happy_path_test.clj` with `(run-tests)` to
check your work as you go.
