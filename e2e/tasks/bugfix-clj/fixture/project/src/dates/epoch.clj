
;; Proleptic-Gregorian civil date <-> days-since-1970-01-01 conversion
;; (Howard Hinnant's well-known integer algorithm). Valid for dates on or
;; after 1970-01-01 -- this library does not support dates before the epoch.

(defn epoch-leap-year? [y]
  (and (zero? (mod y 4))
       (or (not (zero? (mod y 100)))
           (zero? (mod y 400)))))

(defn epoch-days-in-month [y m]
  (nth [31 (if (epoch-leap-year? y) 29 28) 31 30 31 30 31 31 30 31 30 31] (dec m)))

(defn epoch-days-from-civil
  "Days since 1970-01-01 for proleptic-Gregorian year/month/day (month 1-12)."
  [y m d]
  (let [y (if (<= m 2) (dec y) y)
        era (quot y 400)
        yoe (- y (* era 400))
        mp (if (> m 2) (- m 3) (+ m 9))
        doy (+ (quot (+ (* 153 mp) 2) 5) d -1)
        doe (+ (* yoe 365) (quot yoe 4) (- (quot yoe 100)) doy)]
    (+ (* era 146097) doe -719468)))

(defn epoch-civil-from-days
  "Inverse of epoch-days-from-civil: returns [year month day]."
  [z]
  (let [z (+ z 719468)
        era (quot z 146097)
        doe (- z (* era 146097))
        a (quot doe 1460)
        b (quot doe 36524)
        c (quot doe 146096)
        yoe (quot (- (+ doe b) a c) 365)
        y (+ yoe (* era 400))
        doy (- doe (- (+ (* 365 yoe) (quot yoe 4)) (quot yoe 100)))
        mp (quot (+ (* 5 doy) 2) 153)
        d (+ (- doy (quot (+ (* 153 mp) 2) 5)) 1)
        m (if (< mp 10) (+ mp 3) (- mp 9))
        y (if (<= m 2) (inc y) y)]
    [y m d]))

(defn epoch-seconds-of-local
  "Local wall-clock instant (year month day hour minute second) as seconds
  since 1970-01-01T00:00:00, ignoring timezone -- i.e. as if the wall clock
  reading were UTC. Combine with a UTC offset (see dates.zone) to get the
  true UTC instant."
  [y m d hh mm ss]
  (+ (* (epoch-days-from-civil y m d) 86400)
     (* hh 3600)
     (* mm 60)
     ss))
