(ns mnemosyne.shell)

;;; Shell-style file utilities over the async IO substrate.
;;;
;;; Every producer here returns a clojure.core.async channel of logical
;;; elements — lines for `cat`, entry maps for `ls`/`find`, match maps for
;;; `grep` — and every stage takes its channel as the LAST argument, so
;;; pipelines thread with ->>:
;;;
;;;   (->> (find "src" {:name "*.clj" :type :file})
;;;        (grep #"defn")
;;;        (head 10)
;;;        collect <?)
;;;
;;; Transformation between stages is transducer-based: `pipe` applies any
;;; transducer to a channel, and the producers accept an optional transducer
;;; argument as shorthand. Consumption is always asynchronous — sinks like
;;; `collect`/`wc-l`/`write-lines` return a promise channel carrying a single
;;; result.
;;;
;;; Errors travel in-band, following the substrate's convention: an I/O
;;; failure delivers an error value (see clojure.rust.io.async/error?) on the
;;; same channel, and every combinator here passes error values through
;;; untouched — transducers never see them. Take with
;;; clojure.core.async/<? (or <??) to convert an error value into a throw at
;;; the consumption boundary.
;;;
;;; Requires a runtime whose IoPolicy grants file-io: the substrate
;;; namespaces clojure.core.async, clojure.rust.io.async, and
;;; mnemosyne.shell.native must be present.

;; Fail the load immediately when the substrate is absent — symbols inside
;; defn bodies only resolve at call time, and a namespace that half-works
;; under deny-all would be a trap. Evaluating one var from each required
;; namespace errors out here instead.
clojure.core.async/chan
clojure.rust.io.async/line-chan
mnemosyne.shell.native/dir-chan

;;; ── Channel combinators ────────────────────────────────────────────────────

(defn pipe
  "Apply transducer xf to the values of channel in, returning a new channel
  of the transformed values. Honors reduced (early termination): when xf
  stops the reduction — e.g. (take n) — the input channel is closed so
  upstream producers stop working. Error values bypass xf and are forwarded
  as-is."
  [xf in]
  (let [out  (clojure.core.async/chan 8)
        buf  (atom [])
        step (fn ([acc] acc)
               ([acc v] (swap! buf conj v) acc))
        rf   (xf step)
        ;; Forward everything the transducer buffered, preserving order.
        flush! (fn ^:async []
                 (let [vs @buf]
                   (reset! buf [])
                   (loop [xs (seq vs)]
                     (when xs
                       (await (clojure.core.async/put! out (first xs)))
                       (recur (next xs))))))]
    (clojure.core.async/go
      (loop []
        (let [v (await (clojure.core.async/take! in))]
          (if (nil? v)
            (do (rf nil) ; completion: lets stateful xfs flush (partition-all &c.)
                (await (flush!))
                (clojure.core.async/close! out))
            (let [res (if (clojure.rust.io.async/error? v)
                        (do (swap! buf conj v) nil)
                        (rf nil v))]
              (await (flush!))
              (if (reduced? res)
                (do (rf nil)
                    (await (flush!))
                    (clojure.core.async/close! out)
                    (clojure.core.async/close! in))
                (recur)))))))
    out))

(defn collect
  "Drain channel ch into a vector. Returns a promise channel delivering that
  vector when ch closes."
  [ch]
  (let [out (clojure.core.async/chan 1)]
    (clojure.core.async/go
      (let [vs (await (clojure.core.async/reduce conj [] ch))]
        (await (clojure.core.async/put! out vs))
        (clojure.core.async/close! out)))
    out))

(defn tee
  "Split channel ch into n channels (default 2), each receiving every value.
  Returns a vector of the output channels. Backed by core.async mult, so a
  slow consumer applies backpressure to the rest."
  ([ch] (tee ch 2))
  ([ch n]
   (let [m    (clojure.core.async/mult ch)
         outs (vec (map (fn [_] (clojure.core.async/chan 8)) (range n)))]
     (doseq [o outs]
       (clojure.core.async/tap! m o))
     outs)))

;;; ── cat ────────────────────────────────────────────────────────────────────

(defn cat
  "Channel of the lines of the file at path (without trailing newlines).
  With xf, transform the stream: (cat p (filter seq)) skips blank lines."
  ([path] (clojure.rust.io.async/line-chan path))
  ([path xf] (pipe xf (cat path))))

(defn cat*
  "Concatenate the lines of several files into one channel, file order
  preserved."
  ([paths]
   (let [out (clojure.core.async/chan 8)]
     (clojure.core.async/go
       (loop [ps (seq paths)]
         (when ps
           (let [in (clojure.rust.io.async/line-chan (first ps))]
             (loop []
               (let [v (await (clojure.core.async/take! in))]
                 (when (some? v)
                   (await (clojure.core.async/put! out v))
                   (recur))))
             (recur (next ps)))))
       (clojure.core.async/close! out))
     out))
  ([paths xf] (pipe xf (cat* paths))))

;;; ── ls / find / stat ───────────────────────────────────────────────────────

(defn ls
  "Channel of the entries of directory dir, sorted by name. Each entry is a
  map {:path :name :type} with :type one of :file :dir :symlink :other."
  ([dir] (mnemosyne.shell.native/dir-chan dir))
  ([dir xf] (pipe xf (ls dir))))

(defn glob->re
  "Compile a shell glob (\"src/**/*.clj\") to a regex matching the whole
  string: ** crosses path separators, * and ? stay within one segment."
  [glob]
  (let [cs (vec (seq glob))
        n  (count cs)]
    (loop [i 0 out "^"]
      (if (< i n)
        (let [c (nth cs i)]
          (cond
            (and (= c \*) (< (inc i) n) (= (nth cs (inc i)) \*))
            (recur (+ i 2) (str out ".*"))

            (= c \*) (recur (inc i) (str out "[^/]*"))
            (= c \?) (recur (inc i) (str out "[^/]"))

            (contains? #{\. \( \) \[ \] \{ \} \+ \^ \$ \|} c)
            (recur (inc i) (str out "\\" c))

            (= c \\) (recur (inc i) (str out "\\\\"))

            :else (recur (inc i) (str out c))))
        (re-pattern (str out "$"))))))

(defn find
  "Channel of the entries under root (root included), depth-first with
  children in name order. Each entry is {:path :name :type :depth}; symlinks
  are reported but never followed. opts narrow the stream:

    :type       keep entries whose :type matches (:file, :dir, …)
    :name       keep entries whose :name matches this glob string
    :path       keep entries whose :path matches this regex
    :max-depth  keep entries at most this deep (root = 0)
    :xf         extra transducer applied after the filters"
  ([root] (mnemosyne.shell.native/walk-chan root))
  ([root opts]
   (let [type-p  (when (:type opts)
                   (fn [e] (= (:type e) (:type opts))))
         ;; glob->re anchors with ^…$, so re-find is a whole-string match
         ;; here (cljrs's re-matches misses on patterns with escapes).
         name-p  (when (:name opts)
                   (let [re (glob->re (:name opts))]
                     (fn [e] (some? (re-find re (:name e))))))
         path-p  (when (:path opts)
                   (fn [e] (some? (re-find (:path opts) (:path e)))))
         depth-p (when (:max-depth opts)
                   (fn [e] (<= (:depth e) (:max-depth opts))))
         preds   (remove nil? [type-p name-p path-p depth-p])
         base    (if (seq preds)
                   (filter (fn [e] (every? (fn [p] (p e)) preds)))
                   identity)
         xf      (if (:xf opts) (comp base (:xf opts)) base)]
     (pipe xf (mnemosyne.shell.native/walk-chan root)))))

(defn stat
  "Promise channel delivering the metadata map of the entry at path:
  {:path :name :type :size :modified :readonly}. Symlinks are not followed."
  [path]
  (mnemosyne.shell.native/stat path))

(defn ^:async exists?
  "Whether an entry exists at path. Async — use as (await (exists? p))."
  [path]
  (clojure.rust.io.async/ok?
    (await (clojure.core.async/take! (mnemosyne.shell.native/stat path)))))

;;; ── grep ───────────────────────────────────────────────────────────────────

(defn ^:private entry-path
  "The path of a grep target: an entry map's :path, or the value itself."
  [x]
  (if (map? x) (:path x) x))

(defn ^:private grep-target?
  "Whether a value taken from a channel names a file grep should search.
  Entry maps of directories (from find/ls) are skipped."
  [x]
  (if (map? x)
    (= (:type x) :file)
    (string? x)))

(defn ^:async grep-file
  "Grep one file, putting a match map on out for every line matching re.
  Internal worker for grep."
  [re path out]
  (let [in (clojure.rust.io.async/line-chan path)]
    (loop [n 1]
      (let [line (await (clojure.core.async/take! in))]
        (when (some? line)
          (if (clojure.rust.io.async/error? line)
            (await (clojure.core.async/put! out line))
            (let [m (re-find re line)]
              (when m
                (await (clojure.core.async/put!
                         out
                         {:path  path
                          :line  n
                          :text  line
                          :match (if (vector? m) (first m) m)})))))
          (recur (inc n)))))))

(defn grep
  "Channel of match maps {:path :line :text :match} for every line matching
  regex re. src may be a path, a collection of paths, or a channel of paths /
  entry maps (so find pipes straight in — directory entries are skipped).
  Files are searched sequentially, so matches arrive in deterministic order."
  ([re src]
   (let [out (clojure.core.async/chan 8)]
     (clojure.core.async/go
       (cond
         (string? src)
         (await (grep-file re src out))

         (sequential? src)
         (loop [ps (seq src)]
           (when ps
             (await (grep-file re (entry-path (first ps)) out))
             (recur (next ps))))

         :else ; channel
         (loop []
           (let [v (await (clojure.core.async/take! src))]
             (when (some? v)
               (cond
                 (clojure.rust.io.async/error? v)
                 (await (clojure.core.async/put! out v))

                 (grep-target? v)
                 (await (grep-file re (entry-path v) out))

                 :else nil)
               (recur)))))
       (clojure.core.async/close! out))
     out))
  ([re src xf] (pipe xf (grep re src))))

;;; ── head / tail / wc / sed / sort / uniq ───────────────────────────────────

(defn head
  "Channel of the first n values of ch. Closes ch once n values have
  arrived, so upstream work stops early."
  [n ch]
  (pipe (take n) ch))

(defn tail
  "Channel of the last n values of ch. Buffers ch fully (the last n are not
  known until it closes)."
  [n ch]
  (let [out (clojure.core.async/chan 8)]
    (clojure.core.async/go
      (let [vs (await (clojure.core.async/reduce conj [] ch))]
        (loop [xs (seq (take-last n vs))]
          (when xs
            (await (clojure.core.async/put! out (first xs)))
            (recur (next xs))))
        (clojure.core.async/close! out)))
    out))

(defn wc-l
  "Promise channel delivering the number of values on ch (for a cat channel,
  its line count)."
  [ch]
  (let [out (clojure.core.async/chan 1)]
    (clojure.core.async/go
      (let [n (await (clojure.core.async/reduce (fn [acc _] (inc acc)) 0 ch))]
        (await (clojure.core.async/put! out n))
        (clojure.core.async/close! out)))
    out))

(defn sed
  "Channel of (f v) for each value of ch, dropping nils — a per-line rewrite
  stage: (sed #(when-not (starts-with? % \";\") %) (cat p)) strips comment
  lines. Sugar for (pipe (keep f) ch)."
  [f ch]
  (pipe (keep f) ch))

(defn sort-ch
  "Channel of ch's values sorted (by keyfn if given). Buffers ch fully."
  ([ch] (sort-ch identity ch))
  ([keyfn ch]
   (let [out (clojure.core.async/chan 8)]
     (clojure.core.async/go
       (let [vs (await (clojure.core.async/reduce conj [] ch))]
         (loop [xs (seq (sort-by keyfn vs))]
           (when xs
             (await (clojure.core.async/put! out (first xs)))
             (recur (next xs))))
         (clojure.core.async/close! out)))
     out)))

(defn uniq
  "Channel of ch's values with consecutive duplicates collapsed (like the
  uniq program; sort first for global dedup)."
  [ch]
  (pipe (dedupe) ch))

;;; ── Sinks ──────────────────────────────────────────────────────────────────

(defn write-lines
  "Drain ch and write its values to the file at path, one per line
  (trailing newline included). Returns a promise channel delivering the byte
  count written, or the first error value encountered on ch."
  [path ch]
  (let [out (clojure.core.async/chan 1)]
    (clojure.core.async/go
      (let [vs  (await (clojure.core.async/reduce conj [] ch))
            err (first (filter clojure.rust.io.async/error? vs))
            res (if err
                  err
                  (let [text (if (empty? vs) "" (str (join "\n" vs) "\n"))]
                    (await (clojure.core.async/take!
                             (clojure.rust.io.async/spit path text)))))]
        (await (clojure.core.async/put! out res))
        (clojure.core.async/close! out)))
    out))

(defn cp
  "Copy the file at src to dst (byte-for-byte). Returns a promise channel
  delivering the byte count written, or an error value."
  [src dst]
  (let [out (clojure.core.async/chan 1)]
    (clojure.core.async/go
      (let [data (await (clojure.core.async/take!
                          (clojure.rust.io.async/slurp-bytes src)))
            res  (if (clojure.rust.io.async/error? data)
                   data
                   (await (clojure.core.async/take!
                            (clojure.rust.io.async/spit dst data))))]
        (await (clojure.core.async/put! out res))
        (clojure.core.async/close! out)))
    out))
