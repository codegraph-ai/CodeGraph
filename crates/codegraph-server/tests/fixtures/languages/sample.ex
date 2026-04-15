defmodule MyApp.User do
  @moduledoc "User module"
  import Ecto.Query
  alias MyApp.Repo

  @doc "Creates a user"
  def create(name, email) do
    %{name: name, email: email}
  end

  defp validate(user) do
    if user.name == "" do
      {:error, "name required"}
    else
      {:ok, user}
    end
  end
end
