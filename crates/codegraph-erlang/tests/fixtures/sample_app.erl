%% @doc Sample Erlang application for CodeGraph parser tests
-module(sample_app).

-behaviour(gen_server).

-export([start_link/0, stop/0, create_user/2, get_user/1, add_role/2]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2, terminate/2]).

-import(lists, [member/2, filter/2]).

-record(user, {id, name, email, roles = []}).
-record(state, {users = #{}}).

%% ============================================================
%% Public API
%% ============================================================

%% @doc Start the gen_server
start_link() ->
    gen_server:start_link({local, ?MODULE}, ?MODULE, [], []).

%% @doc Stop the server
stop() ->
    gen_server:stop(?MODULE).

%% @doc Create a new user
create_user(Name, Email) ->
    gen_server:call(?MODULE, {create_user, Name, Email}).

%% @doc Get a user by id
get_user(Id) ->
    gen_server:call(?MODULE, {get_user, Id}).

%% @doc Add a role to a user
add_role(UserId, Role) ->
    gen_server:call(?MODULE, {add_role, UserId, Role}).

%% ============================================================
%% gen_server callbacks
%% ============================================================

init([]) ->
    {ok, #state{}}.

handle_call({create_user, Name, Email}, _From, State) ->
    Id = make_ref(),
    User = #user{id = Id, name = Name, email = Email},
    NewState = State#state{users = maps:put(Id, User, State#state.users)},
    {reply, {ok, Id}, NewState};
handle_call({get_user, Id}, _From, State) ->
    case maps:find(Id, State#state.users) of
        {ok, User} -> {reply, {ok, User}, State};
        error      -> {reply, {error, not_found}, State}
    end;
handle_call({add_role, UserId, Role}, _From, State) ->
    case maps:find(UserId, State#state.users) of
        {ok, User} ->
            case has_role(User, Role) of
                true ->
                    {reply, {error, already_has_role}, State};
                false ->
                    Updated = User#user{roles = [Role | User#user.roles]},
                    NewState = State#state{users = maps:put(UserId, Updated, State#state.users)},
                    {reply, ok, NewState}
            end;
        error ->
            {reply, {error, not_found}, State}
    end;
handle_call(_Request, _From, State) ->
    {reply, {error, unknown_request}, State}.

handle_cast(_Msg, State) ->
    {noreply, State}.

handle_info(_Info, State) ->
    {noreply, State}.

terminate(_Reason, _State) ->
    ok.

%% ============================================================
%% Internal helpers
%% ============================================================

%% @doc Check whether a user has a given role
has_role(#user{roles = Roles}, Role) ->
    member(Role, Roles).

%% @doc Filter users by a predicate
filter_users(Pred, Users) ->
    filter(Pred, maps:values(Users)).
