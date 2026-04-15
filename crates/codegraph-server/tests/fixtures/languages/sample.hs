module MyApp.User where

import Data.Text (Text)
import qualified Data.Map as Map

data User = User
  { userName :: Text
  , userEmail :: Text
  }

createUser :: Text -> Text -> User
createUser name email = User name email

greet :: User -> Text
greet user = "Hello, " <> userName user
