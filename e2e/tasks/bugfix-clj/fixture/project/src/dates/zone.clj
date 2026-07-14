
;; Fixed UTC-offset handling. An offset is "minutes east of UTC": local time
;; = UTC + offset, e.g. US Eastern Standard Time is -300 (UTC-5), Japan
;; Standard Time is +540 (UTC+9).

(defn zone-parse-offset-minutes
  "Parse an ISO-8601 offset component (\"Z\", \"+HH:MM\", or \"-HH:MM\") into
  minutes east of UTC."
  [s]
  (if (= s "Z")
    0
    (let [sign (if (= (subs s 0 1) "-") -1 1)
          hh (parse-long (subs s 1 3))
          mm (parse-long (subs s 4 6))]
      (* sign (+ (* hh 60) mm)))))

(defn zone-to-utc-epoch-seconds
  "Convert a local wall-clock reading (as epoch seconds ignoring timezone,
  see dates.epoch/epoch-seconds-of-local) plus its UTC offset in minutes
  into the true UTC epoch-seconds instant."
  [local-epoch-seconds offset-minutes]
  (+ local-epoch-seconds (* offset-minutes 60)))
