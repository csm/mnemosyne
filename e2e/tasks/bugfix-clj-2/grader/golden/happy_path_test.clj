
(deftest test-leap-year
  (is (epoch-leap-year? 2024))
  (is (not (epoch-leap-year? 2023))))

(deftest test-days-round-trip
  (is (= [2024 6 1] (epoch-civil-from-days (epoch-days-from-civil 2024 6 1)))))

(deftest test-slot-make
  (let [s (weekly-slot-make 0 540 600)]
    (is (= 0 (:day-of-week s)))
    (is (= 540 (:start-minute s)))
    (is (= 600 (:end-minute s)))))

(deftest test-occurs-on-is-self-consistent
  ;; Whatever `weekly-day-of-week` computes for a given date, a slot built
  ;; with that same day-of-week should recur on that date.
  (let [d (epoch-days-from-civil 2024 6 1)
        slot (weekly-slot-make (weekly-day-of-week d) 540 600)]
    (is (weekly-slot-occurs-on? slot d))))

(deftest test-occurs-on-rejects-other-days
  (let [d (epoch-days-from-civil 2024 6 1)
        wrong-dow (mod (inc (weekly-day-of-week d)) 7)
        slot (weekly-slot-make wrong-dow 540 600)]
    (is (not (weekly-slot-occurs-on? slot d)))))

(deftest test-conflicts-simple-overlap
  (let [a (weekly-slot-make 0 540 600)
        b (weekly-slot-make 0 570 630)]
    (is (weekly-slot-conflicts? a b))))

(deftest test-conflicts-different-days-never-conflict
  (let [a (weekly-slot-make 0 540 600)
        b (weekly-slot-make 1 540 600)]
    (is (not (weekly-slot-conflicts? a b)))))

(deftest test-no-conflict-when-disjoint-same-day
  (let [a (weekly-slot-make 0 540 600)
        b (weekly-slot-make 0 700 760)]
    (is (not (weekly-slot-conflicts? a b)))))
