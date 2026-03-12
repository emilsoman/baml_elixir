defmodule BamlElixir.TypeBuilder do
  defmodule Class do
    defstruct [:name, :fields]
  end

  defmodule Enum do
    defstruct [:name, :values]
  end

  defmodule EnumValue do
    defstruct [:value, :description, :alias, :skip]
  end

  defmodule Field do
    defstruct [:name, :type, :description, :alias, :skip]
  end

  defmodule Union do
    defstruct [:types]
  end

  defmodule Literal do
    defstruct [:value]
  end

  defmodule Map do
    defstruct [:key_type, :value_type]
  end

  defmodule List do
    defstruct [:type]
  end
end
