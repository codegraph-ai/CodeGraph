module SampleApp exposing (main, Model, Msg, init, update, view)

{-| A sample Elm application demonstrating the features that the
CodeGraph Elm parser should be able to extract.
-}

import Browser
import Html exposing (Html, button, div, h1, p, text)
import Html.Attributes exposing (class, style)
import Html.Events exposing (onClick)
import Http
import Json.Decode exposing (Decoder, field, int, string)


-- TYPES


type Msg
    = Increment
    | Decrement
    | Reset
    | FetchData
    | GotData (Result Http.Error UserData)


type alias Model =
    { count : Int
    , name : String
    , status : Status
    , users : List UserData
    }


type Status
    = Loading
    | Loaded
    | Failed String


type alias UserData =
    { id : Int
    , username : String
    , email : String
    }


-- PORTS


port sendMessage : String -> Cmd msg

port receiveMessage : (String -> msg) -> Sub msg


-- INIT


init : () -> ( Model, Cmd Msg )
init _ =
    ( { count = 0
      , name = "World"
      , status = Loaded
      , users = []
      }
    , Cmd.none
    )


-- UPDATE


update : Msg -> Model -> ( Model, Cmd Msg )
update msg model =
    case msg of
        Increment ->
            ( { model | count = model.count + 1 }, Cmd.none )

        Decrement ->
            ( { model | count = model.count - 1 }, Cmd.none )

        Reset ->
            ( { model | count = 0 }, Cmd.none )

        FetchData ->
            ( { model | status = Loading }, fetchUsers )

        GotData (Ok users) ->
            ( { model | status = Loaded, users = users }, Cmd.none )

        GotData (Err _) ->
            ( { model | status = Failed "Failed to fetch data" }, Cmd.none )


-- VIEW


view : Model -> Html Msg
view model =
    div [ class "app" ]
        [ h1 [] [ text ("Hello, " ++ model.name ++ "!") ]
        , p [] [ text ("Count: " ++ String.fromInt model.count) ]
        , button [ onClick Increment ] [ text "+" ]
        , button [ onClick Decrement ] [ text "-" ]
        , button [ onClick Reset ] [ text "Reset" ]
        , viewStatus model.status
        ]


viewStatus : Status -> Html Msg
viewStatus status =
    case status of
        Loading ->
            p [] [ text "Loading..." ]

        Loaded ->
            p [] [ text "Ready" ]

        Failed err ->
            p [ style "color" "red" ] [ text err ]


-- HTTP


fetchUsers : Cmd Msg
fetchUsers =
    Http.get
        { url = "https://api.example.com/users"
        , expect = Http.expectJson GotData usersDecoder
        }


usersDecoder : Decoder (List UserData)
usersDecoder =
    Json.Decode.list userDecoder


userDecoder : Decoder UserData
userDecoder =
    Json.Decode.map3 UserData
        (field "id" int)
        (field "username" string)
        (field "email" string)


-- HELPERS


formatCount : Int -> String
formatCount n =
    if n == 0 then
        "zero"
    else if n > 0 then
        "positive: " ++ String.fromInt n
    else
        "negative: " ++ String.fromInt n


clampCount : Int -> Int -> Int -> Int
clampCount minVal maxVal n =
    if n < minVal then
        minVal
    else if n > maxVal then
        maxVal
    else
        n


-- MAIN


main : Program () Model Msg
main =
    Browser.element
        { init = init
        , update = update
        , view = view
        , subscriptions = \_ -> receiveMessage (\_ -> Reset)
        }
