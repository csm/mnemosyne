
;; --- bug 1: off-by-one at touching interval boundaries -------------------

(deftest test-interval-touching-is-not-overlap
  (is (not (interval-overlaps? (interval-make 0 100) (interval-make 100 200)))))

(deftest test-interval-one-second-overlap-is-overlap
  (is (interval-overlaps? (interval-make 0 100) (interval-make 99 200))))

(deftest test-interval-intersect-touching-is-nil
  (is (nil? (interval-intersect (interval-make 0 100) (interval-make 100 200)))))

;; --- bug 2: UTC-offset sign error -----------------------------------------

(deftest test-parse-negative-offset
  (is (= 1704092400 (iso-parse "2024-01-01T02:00:00-05:00"))))

(deftest test-parse-positive-offset
  (is (= 1704060000 (iso-parse "2024-01-01T07:00:00+09:00"))))

(deftest test-parse-offset-matches-equivalent-utc
  (is (= (iso-parse "2024-01-01T12:00:00Z")
         (iso-parse "2024-01-01T07:00:00-05:00"))))
