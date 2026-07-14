
;; --- bug 1: day-of-week off-by-one against the true 1970-01-01 Thursday ---

(deftest test-epoch-is-thursday
  (is (= 3 (weekly-day-of-week (epoch-days-from-civil 1970 1 1)))))

(deftest test-known-saturday
  ;; 2024-06-01 is a Saturday.
  (is (= 5 (weekly-day-of-week (epoch-days-from-civil 2024 6 1)))))

(deftest test-known-monday
  ;; 2024-06-03 is a Monday.
  (is (= 0 (weekly-day-of-week (epoch-days-from-civil 2024 6 3)))))

;; --- bug 2: overlap check ignores midnight wraparound ----------------------

(deftest test-wrapping-slot-overlaps-early-morning-slot
  (let [overnight (weekly-slot-make 0 1380 60)     ; 23:00 -> 01:00
        early     (weekly-slot-make 0 0 30)]        ; 00:00 -> 00:30
    (is (weekly-slot-conflicts? overnight early))))

(deftest test-wrapping-slot-does-not-overlap-midday-slot
  (let [overnight (weekly-slot-make 0 1380 60)
        midday    (weekly-slot-make 0 720 780)]
    (is (not (weekly-slot-conflicts? overnight midday)))))

(deftest test-two-wrapping-slots-on-same-day-always-conflict
  ;; Two slots that both wrap past midnight on the same day-of-week always
  ;; share the midnight instant.
  (let [a (weekly-slot-make 0 1380 30)
        b (weekly-slot-make 0 1400 20)]
    (is (weekly-slot-conflicts? a b))))
