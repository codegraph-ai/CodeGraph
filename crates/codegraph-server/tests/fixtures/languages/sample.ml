open Printf

type user = {
  name: string;
  email: string;
}

let create_user name email =
  { name; email }

let greet user =
  if user.name = "" then
    printf "Hello, stranger\n"
  else
    printf "Hello, %s\n" user.name
