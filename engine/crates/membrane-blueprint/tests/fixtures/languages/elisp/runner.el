(defun run (n)
  "Increment N."
  (+ n 1))

(defmacro double-it (n)
  `(* 2 ,n))
