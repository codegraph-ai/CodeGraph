defmodule MyApp.UserController do
  import MyApp.Repo
  alias MyApp.User

  def index do
    users = get(User, :all)
    {:ok, users}
  end

  def show(id) do
    case get(User, id) do
      nil -> {:error, :not_found}
      user -> {:ok, user}
    end
  end

  def create(params) do
    user = User.create(params.name, params.email)
    insert(user)
  end
end
