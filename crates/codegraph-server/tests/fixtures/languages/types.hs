module MyApp.Types where

import Data.Text (Text)

data Role = Admin | Editor | Viewer
  deriving (Show, Eq)

data UserWithRole = UserWithRole
  { uwrName  :: Text
  , uwrEmail :: Text
  , uwrRole  :: Role
  }

class HasName a where
  getName :: a -> Text

instance HasName UserWithRole where
  getName = uwrName

isAdmin :: UserWithRole -> Bool
isAdmin u = uwrRole u == Admin
