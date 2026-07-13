
(deftest test-leap-year
  (is (epoch-leap-year? 2024))
  (is (not (epoch-leap-year? 2023))))

(deftest test-days-round-trip
  (is (= [2024 6 1] (epoch-civil-from-days (epoch-days-from-civil 2024 6 1)))))

(deftest test-parse-utc
  (is (= 1717254000 (iso-parse "2024-06-01T15:00:00Z"))))

(deftest test-format-round-trip
  (is (= "2024-06-01T15:00:00Z" (iso-format (iso-parse "2024-06-01T15:00:00Z")))))

(deftest test-interval-basics
  (let [iv (interval-make 0 100)]
    (is (= 100 (interval-duration-seconds iv)))
    (is (interval-contains-instant? iv 50))
    (is (not (interval-contains-instant? iv 100)))))

(deftest test-interval-overlap-clear-cases
  (is (interval-overlaps? (interval-make 0 100) (interval-make 50 150)))
  (is (not (interval-overlaps? (interval-make 0 100) (interval-make 200 300)))))
