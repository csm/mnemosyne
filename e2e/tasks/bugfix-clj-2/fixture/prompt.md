There is a small Clojure library at `/task/project` (a weekly recurring
time-slot library built on the same epoch-day arithmetic you may have seen
before, see `/task/project/README.md` for the file layout and load order).
Users report two problems:

1. The day-of-week computed for a given date is sometimes wrong.
2. A recurring slot that spans midnight (e.g. 23:00 to 01:00) doesn't
   correctly conflict with a slot in the early-morning hours it actually
   overlaps.

Find and fix the bugs. Do not modify anything under `/task/project/test`.

This is exactly the kind of code `clojure_eval`'s persistent runtime is
good at: load the source files in the order `README.md` describes, then
load and run `/task/project/test/happy_path_test.clj` with `(run-tests)`
to check your work as you go. If you have functions saved from a similar
task, check whether any of them apply here before re-deriving them.
