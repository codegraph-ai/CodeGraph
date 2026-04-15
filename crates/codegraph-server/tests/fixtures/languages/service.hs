module MyApp.Service where

import MyApp.Types (UserWithRole(..), Role(..), HasName, getName, isAdmin)
import MyApp.User (User(..), createUser, greet)
import Data.Text (Text, pack)

createAdmin :: Text -> Text -> UserWithRole
createAdmin name email = UserWithRole name email Admin

promote :: UserWithRole -> UserWithRole
promote u = u { uwrRole = Admin }

displayUser :: UserWithRole -> Text
displayUser u = getName u <> " (" <> pack (show (uwrRole u)) <> ")"
