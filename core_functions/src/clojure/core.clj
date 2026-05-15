(ns mnemosyne.core)

;;; String utilities

(defn str-join
  "Join a collection of strings with sep."
  [sep coll]
  (clojure.string/join sep coll))

(defn str-split-lines
  "Split s into a vector of lines."
  [s]
  (clojure.string/split-lines s))

(defn str-trim [s] (clojure.string/trim s))

;;; Collection utilities

(defn index-by
  "Build a map from (f item) → item for each item in coll."
  [f coll]
  (into {} (map (juxt f identity) coll)))

(defn group-by-first
  "Like group-by but keeps only the first match per key."
  [f coll]
  (reduce (fn [m item]
            (let [k (f item)]
              (if (contains? m k) m (assoc m k item))))
          {}
          coll))

(defn deep-merge
  "Recursively merge maps."
  [& maps]
  (apply merge-with
         (fn [a b]
           (if (and (map? a) (map? b))
             (deep-merge a b)
             b))
         maps))

;;; I/O helpers

(defn slurp-lines
  "Read a file and return its lines as a vector."
  [path]
  (str-split-lines (slurp path)))
