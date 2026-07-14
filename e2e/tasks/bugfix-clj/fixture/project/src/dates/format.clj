
(defn- pad2 [n]
  (if (< n 10) (str "0" n) (str n)))

(defn iso-format
  "Format UTC epoch-seconds as an ISO-8601 UTC timestamp, e.g.
  \"2024-06-01T15:00:00Z\"."
  [epoch-seconds]
  (let [days (quot epoch-seconds 86400)
        sec-of-day (- epoch-seconds (* days 86400))
        [y m d] (epoch-civil-from-days days)
        hh (quot sec-of-day 3600)
        mm (quot (- sec-of-day (* hh 3600)) 60)
        ss (- sec-of-day (* hh 3600) (* mm 60))]
    (str y "-" (pad2 m) "-" (pad2 d) "T" (pad2 hh) ":" (pad2 mm) ":" (pad2 ss) "Z")))
