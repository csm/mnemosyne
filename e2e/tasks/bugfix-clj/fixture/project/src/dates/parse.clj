
(def iso-pattern
  #"^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(Z|[+-]\d{2}:\d{2})$")

(defn iso-parse
  "Parse an ISO-8601 timestamp (e.g. \"2024-06-01T15:00:00Z\" or
  \"2024-06-01T15:00:00-05:00\") into UTC epoch-seconds. Returns nil if `s`
  does not match."
  [s]
  (let [m (re-find iso-pattern s)]
    (when m
      (let [[_ y mo d h mi se off] m
            local (epoch-seconds-of-local
                    (parse-long y) (parse-long mo) (parse-long d)
                    (parse-long h) (parse-long mi) (parse-long se))
            offset-minutes (zone-parse-offset-minutes off)]
        (zone-to-utc-epoch-seconds local offset-minutes)))))
