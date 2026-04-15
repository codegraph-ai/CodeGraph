defmodule MyApp.Repo do
  @moduledoc "Database repository"

  def get(schema, id) do
    %{id: id, schema: schema}
  end

  def insert(changeset) do
    {:ok, changeset}
  end

  def delete(record) do
    {:ok, record}
  end
end
