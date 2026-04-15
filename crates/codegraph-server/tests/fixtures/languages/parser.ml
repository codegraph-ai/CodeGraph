open Printf

type token =
  | Int of int
  | Plus
  | Minus
  | Star
  | Lparen
  | Rparen

type expr =
  | Num of int
  | BinOp of string * expr * expr

let tokenize input =
  let tokens = ref [] in
  String.iter (fun c ->
    match c with
    | '+' -> tokens := Plus :: !tokens
    | '-' -> tokens := Minus :: !tokens
    | '*' -> tokens := Star :: !tokens
    | '(' -> tokens := Lparen :: !tokens
    | ')' -> tokens := Rparen :: !tokens
    | c when c >= '0' && c <= '9' ->
      tokens := Int (Char.code c - Char.code '0') :: !tokens
    | _ -> ()
  ) input;
  List.rev !tokens

let rec eval_expr = function
  | Num n -> n
  | BinOp ("+", l, r) -> eval_expr l + eval_expr r
  | BinOp ("-", l, r) -> eval_expr l - eval_expr r
  | BinOp ("*", l, r) -> eval_expr l * eval_expr r
  | BinOp (_, _, _) -> 0

let print_result expr =
  printf "Result: %d\n" (eval_expr expr)
