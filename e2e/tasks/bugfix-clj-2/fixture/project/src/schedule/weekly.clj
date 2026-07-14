
;; Weekly recurring time-of-day slots: a "slot" is a day-of-week (Monday=0
;; .. Sunday=6) plus a half-open [start-minute, end-minute) window in
;; minute-of-day (0-1439). Built on dates.epoch's day numbering (see
;; dates/epoch.clj, which must be loaded first).

(defn weekly-slot-make
  "A recurring weekly slot: `day-of-week` is Monday=0 .. Sunday=6;
  `start-minute`/`end-minute` are minute-of-day, half-open [start, end)."
  [day-of-week start-minute end-minute]
  {:day-of-week day-of-week :start-minute start-minute :end-minute end-minute})

(defn weekly-day-of-week
  "Day of week (Monday=0 .. Sunday=6) for `epoch-days` (days since
  1970-01-01, see dates.epoch/epoch-days-from-civil)."
  [epoch-days]
  (mod (+ epoch-days 2) 7))

(defn weekly-slot-occurs-on?
  "Does `slot` recur on the day `epoch-days` falls on?"
  [slot epoch-days]
  (= (:day-of-week slot) (weekly-day-of-week epoch-days)))

(defn weekly-minutes-overlap?
  "Do two half-open minute-of-day windows [s1,e1) and [s2,e2) overlap?"
  [s1 e1 s2 e2]
  (and (< s1 e2) (< s2 e1)))

(defn weekly-slot-conflicts?
  "Do two weekly slots conflict: same day-of-week and overlapping
  minute-of-day windows?"
  [a b]
  (and (= (:day-of-week a) (:day-of-week b))
       (weekly-minutes-overlap? (:start-minute a) (:end-minute a)
                                 (:start-minute b) (:end-minute b))))
