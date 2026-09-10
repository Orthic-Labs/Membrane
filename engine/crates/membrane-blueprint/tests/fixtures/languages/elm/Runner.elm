module Runner exposing (run, start)


run : Int -> Int
run n =
    n + 1


start : Int
start =
    run 41


type alias Config =
    { name : String }


type Status
    = Ready
    | Done
