(ns mnemosyne.templates)

;;; Template functions serve as building blocks for the structural editor.
;;; Each template is a valid Clojure function that the editor can clone
;;; and specialise by substituting the annotated slots.

(defn template:transform-coll
  "TEMPLATE — transform every element of a collection.
   Slots: :pred (filter predicate), :xform (element transform)."
  [coll]
  (->> coll
       (filter :pred)         ; slot: replace with concrete predicate
       (map :xform)))         ; slot: replace with concrete transform

(defn template:reduce-to-map
  "TEMPLATE — fold a collection into a map.
   Slots: :key-fn, :val-fn."
  [coll]
  (reduce (fn [acc item]
            (assoc acc (:key-fn item) (:val-fn item)))
          {}
          coll))

(defn template:retry
  "TEMPLATE — retry a thunk up to n times on exception.
   Slots: :n (retry count), :thunk (zero-arg fn)."
  [n thunk]
  (loop [attempts n]
    (or (try (:thunk) (catch Exception _ nil))
        (when (pos? attempts)
          (recur (dec attempts))))))

(defn template:pipeline
  "TEMPLATE — thread a value through an ordered list of steps.
   Slots: :steps (seq of single-arg fns)."
  [value steps]
  (reduce (fn [v step] (step v)) value steps))
