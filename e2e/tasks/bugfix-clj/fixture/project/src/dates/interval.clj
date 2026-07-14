
(defn interval-make
  "A half-open interval [start, end) of epoch-seconds."
  [start end]
  (when (< end start)
    (throw (ex-info "interval end must not precede start" {:start start :end end})))
  {:start start :end end})

(defn interval-duration-seconds [iv]
  (- (:end iv) (:start iv)))

(defn interval-contains-instant?
  [iv t]
  (and (<= (:start iv) t) (< t (:end iv))))

(defn interval-overlaps?
  "Do two half-open intervals share any instant? Intervals that merely
  touch (one's end equals the other's start) do not overlap."
  [a b]
  (and (<= (:start a) (:end b)) (<= (:start b) (:end a))))

(defn interval-intersect
  "The overlapping portion of two intervals, or nil if they don't overlap."
  [a b]
  (when (interval-overlaps? a b)
    (interval-make (max (:start a) (:start b)) (min (:end a) (:end b)))))
